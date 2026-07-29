//! Bounded sleeping worker for the initial domain's software local link.

use anemone_net_api::Recheck;

use crate::{
    net::domain::LocalPumpPort,
    prelude::*,
    task::kthread::{KThreadBuilder, KThreadCtx},
    utils::any_opaque::AnyOpaque,
};

use super::{PUMP_BUDGET, PumpControl, deadline_due, network_now, schedule_deadline};

pub(in crate::net) struct PreparedLocalPath {
    control: Arc<PumpControl>,
    interface: anemone_net_api::InterfaceId,
}

impl PreparedLocalPath {
    pub(in crate::net) fn control(&self) -> Arc<PumpControl> {
        self.control.clone()
    }

    pub(in crate::net) const fn interface(&self) -> anemone_net_api::InterfaceId {
        self.interface
    }

    pub(in crate::net) fn activate(&self) {
        assert!(
            !self.control.active.swap(true, Ordering::AcqRel),
            "local network worker activated twice"
        );
        self.control.request_work();
    }
}

#[derive(Opaque)]
struct LocalWorkerArg {
    port: LocalPumpPort,
    control: Arc<PumpControl>,
}

pub(in crate::net) fn prepare_local(port: LocalPumpPort) -> PreparedLocalPath {
    let interface = port.interface();
    let control = Arc::new(PumpControl::new());
    let worker = KThreadBuilder::new("net:lo".to_string())
        .spawn(
            local_worker_entry,
            AnyOpaque::new(LocalWorkerArg {
                port,
                control: control.clone(),
            }),
        )
        .expect("the mandatory initial-domain local worker must spawn");
    control.install_worker(worker);
    PreparedLocalPath { control, interface }
}

fn local_worker_entry(ctx: KThreadCtx, arg: AnyOpaque) -> i32 {
    let arg = arg
        .cast::<LocalWorkerArg>()
        .expect("local network worker received invalid private data");
    let control = arg.control.clone();
    let mut immediate_repoll = false;
    let mut next_deadline = None;
    let mut armed_deadline = None;

    'worker: loop {
        ctx.wait_until(|| {
            control.active.load(Ordering::Acquire)
                && (control.work_requested() || immediate_repoll || deadline_due(next_deadline))
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

        let mut rounds = 0;
        loop {
            let outcome = arg
                .port
                .pump(network_now(), PUMP_BUDGET)
                .expect("active local worker lost its Stack mapping");
            rounds += 1;
            immediate_repoll = outcome.recheck == Recheck::Immediate;
            next_deadline = outcome.next_deadline;

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
        } else if let Some(deadline) = next_deadline
            && armed_deadline != Some(deadline)
        {
            schedule_deadline(control.clone(), deadline);
            armed_deadline = Some(deadline);
        }
    }

    // DomainStack and local link are boot-persistent. Runtime reclamation is
    // outside R0 and the stopped worker retains only boot-lifetime capabilities.
    0
}
