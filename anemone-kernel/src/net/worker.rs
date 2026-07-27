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

pub(super) enum AttachFailure<P> {
    MissingEthernetAddress(PublishedNetdev<P>),
    WorkerSpawn {
        error: SysError,
        published: PublishedNetdev<P>,
    },
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
/// explicit work, and pump admission; the attach authority lock owns terminal
/// shutdown admission.
pub(super) struct PumpControl {
    worker: spin::Once<KThreadHandle>,
    active: AtomicBool,
    explicit_work: AtomicBool,
}

impl PumpControl {
    fn new() -> Self {
        Self {
            worker: spin::Once::new(),
            active: AtomicBool::new(false),
            explicit_work: AtomicBool::new(false),
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
) -> Result<PreparedPath, AttachFailure<P>> {
    let (snapshot, mut provider) = published.into_parts();
    let Some(ethernet_address) = snapshot.facts().ethernet_address else {
        return Err(AttachFailure::MissingEthernetAddress(
            PublishedNetdev::from_parts(snapshot, provider),
        ));
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
            let published = PublishedNetdev::from_parts(snapshot, core.provider);
            return Err(AttachFailure::WorkerSpawn { error, published });
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

        if ctx.should_stop() {
            break;
        }

        if immediate_repoll {
            control.request_work();
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
