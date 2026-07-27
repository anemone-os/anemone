use anemone_net_api::{Instant as NetworkInstant, InterfaceId, Recheck};
use anemone_smoltcp_stack::{PumpBudget, Stack};

use crate::{
    device::net::{NetdevFrameProvider, NetdevSnapshot, PublishedNetdev, RecheckWake},
    prelude::*,
    task::kthread::{KThreadBuilder, KThreadCtx, KThreadHandle},
    time::timer::schedule_threaded_timer_event,
    utils::any_opaque::AnyOpaque,
};

static_assert!(
    NET_PUMP_INGRESS_BUDGET_FRAMES > 0,
    "NET_PUMP_INGRESS_BUDGET_FRAMES must be non-zero"
);
static_assert!(
    NET_PUMP_EGRESS_BUDGET_STEPS > 0,
    "NET_PUMP_EGRESS_BUDGET_STEPS must be non-zero"
);
static_assert!(
    NET_WORKER_REPOLL_ROUNDS > 0,
    "NET_WORKER_REPOLL_ROUNDS must be non-zero"
);

const PUMP_BUDGET: PumpBudget =
    PumpBudget::new(NET_PUMP_INGRESS_BUDGET_FRAMES, NET_PUMP_EGRESS_BUDGET_STEPS);
#[cfg(feature = "kunit")]
pub(super) const WORKER_REPOLL_LIMIT: usize = NET_WORKER_REPOLL_ROUNDS;

#[derive(Debug)]
pub(super) enum AttachFailure {
    MissingEthernetAddress,
    WorkerSpawn(SysError),
}

pub(super) struct PreparedPath {
    snapshot: NetdevSnapshot,
    interface: InterfaceId,
    control: Arc<PumpControl>,
}

impl PreparedPath {
    pub(super) fn snapshot(&self) -> &NetdevSnapshot {
        &self.snapshot
    }

    pub(super) const fn interface(&self) -> InterfaceId {
        self.interface
    }

    pub(super) fn control(&self) -> Arc<PumpControl> {
        self.control.clone()
    }

    pub(super) fn activate(self) {
        self.control.active.store(true, Ordering::Release);
        self.control.request_work();
    }

    pub(super) fn stop_and_retain(self) {
        self.control.request_shutdown();
    }
}

/// Worker lifecycle and pure wake/work predicates owned by the attach path.
///
/// The provider remains the durable completion/recheck truth and `Stack`
/// remains the protocol/deadline truth. These atomics only project activation,
/// explicit work, and validation requests that originate in this owner; the
/// attach authority lock owns terminal shutdown admission.
pub(super) struct PumpControl {
    worker: spin::Once<KThreadHandle>,
    active: AtomicBool,
    explicit_work: AtomicBool,
    #[cfg(feature = "kunit")]
    probe_requested: AtomicBool,
    #[cfg(feature = "kunit")]
    probe_burst: AtomicUsize,
    #[cfg(feature = "kunit")]
    probe_completed: AtomicBool,
    #[cfg(feature = "kunit")]
    probe_event: Event,
    /// Diagnostic-only mirrors of worker actions. They never decide whether
    /// work, a provider recheck, or a stack deadline is ready.
    #[cfg(feature = "kunit")]
    probe_worker_actions: AtomicUsize,
    #[cfg(feature = "kunit")]
    probe_max_pump_rounds: AtomicUsize,
    #[cfg(feature = "kunit")]
    probe_repoll_requests: AtomicUsize,
    #[cfg(feature = "kunit")]
    probe_yields: AtomicUsize,
}

#[cfg(feature = "kunit")]
#[derive(Clone, Copy, Debug)]
pub(super) struct WorkerProbeStats {
    worker_actions: usize,
    max_pump_rounds: usize,
    repoll_requests: usize,
    yields: usize,
}

#[cfg(feature = "kunit")]
impl WorkerProbeStats {
    pub(super) const fn worker_actions(self) -> usize {
        self.worker_actions
    }
    pub(super) const fn max_pump_rounds(self) -> usize {
        self.max_pump_rounds
    }
    pub(super) const fn repoll_requests(self) -> usize {
        self.repoll_requests
    }
    pub(super) const fn yields(self) -> usize {
        self.yields
    }
}

impl PumpControl {
    fn new() -> Self {
        Self {
            worker: spin::Once::new(),
            active: AtomicBool::new(false),
            explicit_work: AtomicBool::new(false),
            #[cfg(feature = "kunit")]
            probe_requested: AtomicBool::new(false),
            #[cfg(feature = "kunit")]
            probe_burst: AtomicUsize::new(0),
            #[cfg(feature = "kunit")]
            probe_completed: AtomicBool::new(false),
            #[cfg(feature = "kunit")]
            probe_event: Event::new(),
            #[cfg(feature = "kunit")]
            probe_worker_actions: AtomicUsize::new(0),
            #[cfg(feature = "kunit")]
            probe_max_pump_rounds: AtomicUsize::new(0),
            #[cfg(feature = "kunit")]
            probe_repoll_requests: AtomicUsize::new(0),
            #[cfg(feature = "kunit")]
            probe_yields: AtomicUsize::new(0),
        }
    }

    fn install_worker(&self, worker: KThreadHandle) {
        assert!(
            self.worker.get().is_none(),
            "network pump worker installed twice"
        );
        self.worker.call_once(|| worker);
    }

    fn wake_worker(&self) {
        if let Some(worker) = self.worker.get() {
            worker.wake();
        }
    }

    fn request_work(&self) {
        if !self.active.load(Ordering::Acquire) {
            return;
        }
        self.explicit_work.store(true, Ordering::Release);
        // Shutdown closes `active` before clearing explicit work. Rechecking
        // prevents an in-flight requester from restoring work after that
        // linearization point.
        if !self.active.load(Ordering::Acquire) {
            self.explicit_work.store(false, Ordering::Release);
            return;
        }
        self.wake_worker();
    }

    fn work_requested(&self) -> bool {
        self.explicit_work.load(Ordering::Acquire)
    }

    fn take_work_request(&self) -> bool {
        self.explicit_work.swap(false, Ordering::AcqRel)
    }

    pub(super) fn request_shutdown(&self) {
        // Revoke pump admission before touching the worker. A queued timer or
        // recheck edge may still wake it, but cannot reactivate the predicate.
        self.active.store(false, Ordering::Release);
        self.explicit_work.store(false, Ordering::Release);
        self.worker
            .get()
            .expect("prepared network path must have an installed worker")
            .request_stop();
    }

    #[cfg(feature = "kunit")]
    pub(super) fn request_kunit_probe(&self, burst: usize) {
        assert!(burst > 0, "network KUnit probe burst must be non-zero");
        assert!(
            !self.probe_completed.load(Ordering::Acquire),
            "completed network KUnit probe restarted"
        );
        assert_eq!(
            self.probe_burst.swap(burst, Ordering::AcqRel),
            0,
            "network KUnit probe burst installed twice"
        );
        let already_requested = self.probe_requested.swap(true, Ordering::AcqRel);
        assert!(!already_requested, "network KUnit probe requested twice");
        self.request_work();
    }

    #[cfg(feature = "kunit")]
    fn take_kunit_probe_request(&self) -> Option<usize> {
        if !self.probe_requested.swap(false, Ordering::AcqRel) {
            return None;
        }
        let burst = self.probe_burst.swap(0, Ordering::AcqRel);
        assert!(burst > 0, "requested network KUnit probe lost its burst");
        Some(burst)
    }

    #[cfg(feature = "kunit")]
    fn complete_kunit_probe(&self) {
        assert!(
            !self.probe_completed.swap(true, Ordering::AcqRel),
            "network KUnit probe completed twice"
        );
        self.probe_event.publish(usize::MAX, true);
    }

    #[cfg(feature = "kunit")]
    pub(super) fn kunit_probe_completed(&self) -> bool {
        self.probe_completed.load(Ordering::Acquire)
    }

    #[cfg(feature = "kunit")]
    pub(super) fn wait_for_kunit_probe(&self, timeout: Duration) {
        let outcome =
            self.probe_event
                .listen_with_timeout(false, || self.kunit_probe_completed(), timeout);
        assert!(
            !matches!(outcome, Some(TimeoutListenException::Timeout)),
            "RV64 network vertical slice timed out"
        );
        assert!(
            !matches!(outcome, Some(TimeoutListenException::Signaled)),
            "RV64 network vertical slice wait was interrupted"
        );
    }

    #[cfg(feature = "kunit")]
    pub(super) fn worker_probe_stats(&self) -> WorkerProbeStats {
        WorkerProbeStats {
            worker_actions: self.probe_worker_actions.load(Ordering::Relaxed),
            max_pump_rounds: self.probe_max_pump_rounds.load(Ordering::Relaxed),
            repoll_requests: self.probe_repoll_requests.load(Ordering::Relaxed),
            yields: self.probe_yields.load(Ordering::Relaxed),
        }
    }
}

impl RecheckWake for PumpControl {
    fn wake(&self) {
        self.wake_worker();
    }
}

struct PumpCore<P: NetdevFrameProvider> {
    stack: Stack,
    provider: P,
    interface: InterfaceId,
}

struct WorkerLaunch<P: NetdevFrameProvider> {
    core: SpinLock<Option<PumpCore<P>>>,
}

impl<P: NetdevFrameProvider> WorkerLaunch<P> {
    fn new(core: PumpCore<P>) -> Self {
        Self {
            core: SpinLock::new(Some(core)),
        }
    }

    fn take(&self) -> PumpCore<P> {
        self.core
            .lock()
            .take()
            .expect("network pump core was taken more than once")
    }
}

#[derive(Opaque)]
struct WorkerArg<P: NetdevFrameProvider> {
    launch: Arc<WorkerLaunch<P>>,
    control: Arc<PumpControl>,
}

pub(super) fn prepare<P: NetdevFrameProvider>(
    published: PublishedNetdev<P>,
) -> Result<PreparedPath, AttachFailure> {
    let (snapshot, mut provider) = published.into_parts();
    let Some(ethernet_address) = snapshot.facts().ethernet_address else {
        // The IRQ is already registered and the device may still own RX
        // mappings. R0 has no removable IRQ/reset rollback, so retain provider
        // backing to power-off while leaving the registry entry unattached.
        core::mem::forget(provider);
        return Err(AttachFailure::MissingEthernetAddress);
    };

    let mut stack = Stack::new();
    let interface = stack.add_interface(&mut provider, ethernet_address, network_now());
    let control = Arc::new(PumpControl::new());
    let wake: Arc<dyn RecheckWake> = control.clone();
    provider.install_recheck_wake(Arc::downgrade(&wake));
    drop(wake);

    let launch = Arc::new(WorkerLaunch::new(PumpCore {
        stack,
        provider,
        interface,
    }));
    let worker = match KThreadBuilder::new(format!("net:{}", snapshot.name())).spawn(
        network_worker_entry::<P>,
        AnyOpaque::new(WorkerArg {
            launch: launch.clone(),
            control: control.clone(),
        }),
    ) {
        Ok(worker) => worker,
        Err(error) => {
            let mut core = launch.take();
            core.stack
                .remove_interface(interface)
                .expect("failed network attach lost its stack-local mapping");
            // As above, IRQ/device ownership prevents safe provider teardown.
            core::mem::forget(core.provider);
            return Err(AttachFailure::WorkerSpawn(error));
        },
    };
    control.install_worker(worker);

    Ok(PreparedPath {
        snapshot,
        interface,
        control,
    })
}

fn network_worker_entry<P: NetdevFrameProvider>(ctx: KThreadCtx, arg: AnyOpaque) -> i32 {
    let arg = arg
        .cast::<WorkerArg<P>>()
        .expect("network worker received invalid private data");
    let control = arg.control.clone();
    let mut core = arg.launch.take();
    let mut immediate_repoll = false;
    let mut next_deadline = None;
    // Timer callbacks carry only a wake edge and cannot clear this worker-local
    // record. The worker clears a due arm after it wakes, preventing ordinary
    // IRQ/work pumps from enqueueing duplicate callbacks for the same deadline.
    let mut armed_deadline = None;
    #[cfg(feature = "kunit")]
    let mut probe_started = false;

    'worker: loop {
        ctx.wait_until(|| {
            control.active.load(Ordering::Acquire)
                && (control.work_requested()
                    || core.provider.recheck_requested()
                    || immediate_repoll
                    || deadline_due(next_deadline))
        });
        if ctx.should_stop() {
            break;
        }
        if !control.active.load(Ordering::Acquire) {
            continue;
        }

        if armed_deadline.is_some_and(|deadline| deadline <= network_now()) {
            armed_deadline = None;
        }
        control.take_work_request();
        core.provider.take_recheck_requested();

        #[cfg(feature = "kunit")]
        if let Some(burst) = control.take_kunit_probe_request() {
            core.stack
                .start_icmp_echo_probe(core.interface, [10, 0, 2, 15], 24, [10, 0, 2, 2], burst)
                .expect("active network path lost its KUnit interface mapping");
            probe_started = true;
        }

        #[cfg(feature = "kunit")]
        control.probe_worker_actions.fetch_add(1, Ordering::Relaxed);

        let mut rounds = 0;
        loop {
            let now = network_now();
            let outcome = core
                .stack
                .pump(core.interface, &mut core.provider, now, PUMP_BUDGET)
                .expect("active network path lost its interface mapping");
            rounds += 1;
            immediate_repoll = outcome.recheck == Recheck::Immediate;
            next_deadline = outcome.next_deadline;

            #[cfg(feature = "kunit")]
            if probe_started
                && core
                    .stack
                    .icmp_echo_probe_completed(core.interface)
                    .expect("active network path lost its KUnit interface mapping")
            {
                control.complete_kunit_probe();
                probe_started = false;
            }

            // Stop is rechecked between bounded pump rounds. The current
            // callback is allowed to finish, but no later round, repoll, or
            // deadline arm may be initiated after stop is observed.
            if ctx.should_stop() {
                break 'worker;
            }

            if !immediate_repoll || rounds == NET_WORKER_REPOLL_ROUNDS {
                break;
            }
        }

        #[cfg(feature = "kunit")]
        control
            .probe_max_pump_rounds
            .fetch_max(rounds, Ordering::Relaxed);

        if ctx.should_stop() {
            break;
        }

        if immediate_repoll {
            #[cfg(feature = "kunit")]
            control
                .probe_repoll_requests
                .fetch_add(1, Ordering::Relaxed);
            control.request_work();
            #[cfg(feature = "kunit")]
            control.probe_yields.fetch_add(1, Ordering::Relaxed);
            yield_now();
        } else if let Some(deadline) = next_deadline {
            if armed_deadline != Some(deadline) {
                schedule_deadline(control.clone(), deadline);
                armed_deadline = Some(deadline);
            }
        }
    }

    // IRQ registration cannot currently be removed and the VirtIO queues have
    // no reset/quiesce proof. Dropping `PumpCore` here would release the sole
    // provider/slot/DMA owner while the device or IRQ Weak capability could
    // still access it. A future runtime-removal path may delete this retention
    // only after preventing Weak upgrades and proving queue/device quiescence.
    core::mem::forget(core);
    0
}

fn network_now() -> NetworkInstant {
    let micros = crate::time::Instant::now().to_duration().as_micros();
    let micros = i64::try_from(micros).unwrap_or(i64::MAX);
    NetworkInstant::from_micros(micros)
}

fn deadline_due(deadline: Option<NetworkInstant>) -> bool {
    deadline.is_some_and(|deadline| deadline <= network_now())
}

fn schedule_deadline(control: Arc<PumpControl>, deadline: NetworkInstant) {
    if !control.active.load(Ordering::Acquire) {
        return;
    }
    let now = network_now();
    if deadline <= now {
        control.request_work();
        return;
    }
    let delay = u64::try_from(deadline.total_micros() - now.total_micros())
        .expect("future network deadline must have a non-negative duration");
    schedule_threaded_timer_event(
        Duration::from_micros(delay),
        Box::new(move || control.wake_worker()),
    );
}
