//! External-provider pump preparation and worker progression.

use anemone_net_api::{EthernetAddress, InterfaceId, Recheck};

use crate::{
    device::net::{NetdevFrameProvider, NetdevSnapshot, PublishedNetdev, RecheckWake},
    net::domain::{DomainStack, ExternalMapping, ExternalPumpPort},
    prelude::*,
    task::kthread::{KThreadBuilder, KThreadCtx},
    utils::any_opaque::AnyOpaque,
};

use super::{PUMP_BUDGET, PumpControl, deadline_due, network_now, schedule_deadline};

pub(in crate::net) enum AttachFailure<P> {
    WorkerSpawn {
        error: SysError,
        published: PublishedNetdev<P>,
    },
}

pub(in crate::net) struct PreparedPath {
    snapshot: NetdevSnapshot,
    mapping: Option<ExternalMapping>,
    control: Arc<PumpControl>,
}

impl PreparedPath {
    pub(in crate::net) fn snapshot(&self) -> &NetdevSnapshot {
        &self.snapshot
    }

    pub(in crate::net) fn interface(&self) -> InterfaceId {
        self.mapping
            .as_ref()
            .expect("prepared network path lost its mapping")
            .interface()
    }

    pub(in crate::net) fn control(&self) -> Arc<PumpControl> {
        self.control.clone()
    }

    pub(in crate::net) fn activate(mut self) {
        self.mapping
            .take()
            .expect("network path mapping committed twice")
            .commit();
        self.control.active.store(true, Ordering::Release);
        self.control.request_work();
    }

    pub(in crate::net) fn rollback_mapping(&mut self) {
        self.mapping
            .take()
            .expect("network path mapping rolled back twice")
            .rollback();
    }

    pub(in crate::net) fn stop_and_retain(self) {
        assert!(
            self.mapping.is_none(),
            "terminal retention must withdraw the unpublished Stack mapping first"
        );
        self.control.request_shutdown();
    }
}

struct PumpCore<P: NetdevFrameProvider> {
    port: ExternalPumpPort,
    provider: P,
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

pub(in crate::net) fn prepare<P: NetdevFrameProvider>(
    published: PublishedNetdev<P>,
    stack: Arc<DomainStack>,
    ethernet_address: EthernetAddress,
    worker_name: &str,
) -> Result<PreparedPath, AttachFailure<P>> {
    let (snapshot, mut provider) = published.into_parts();
    let mapping = stack.attach_external(&mut provider, ethernet_address, network_now());
    let control = Arc::new(PumpControl::new());
    let wake: Arc<dyn RecheckWake> = control.clone();
    provider.install_recheck_wake(Arc::downgrade(&wake));
    drop(wake);

    let launch = Arc::new(WorkerLaunch::new(PumpCore {
        port: mapping.pump_port(),
        provider,
    }));
    let worker = match KThreadBuilder::new(format!("net:{worker_name}")).spawn(
        network_worker_entry::<P>,
        AnyOpaque::new(WorkerArg {
            launch: launch.clone(),
            control: control.clone(),
        }),
    ) {
        Ok(worker) => worker,
        Err(error) => {
            let core = launch.take();
            mapping.rollback();
            let published = PublishedNetdev::from_parts(snapshot, core.provider);
            return Err(AttachFailure::WorkerSpawn { error, published });
        },
    };
    control.install_worker(worker);

    Ok(PreparedPath {
        snapshot,
        mapping: Some(mapping),
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
                .port
                .pump(&mut core.provider, now, PUMP_BUDGET)
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
