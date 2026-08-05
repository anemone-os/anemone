//! Worker wake, deadline, activation, and stop projection.

use anemone_net_api::Instant as NetworkInstant;

use crate::{
    device::net::RecheckWake, prelude::*, task::kthread::KThreadHandle,
    time::timer::schedule_threaded_timer_event,
};

use super::network_now;

/// Worker lifecycle and pure wake/work predicates owned by the attach path.
///
/// The provider remains the durable completion/recheck truth and the domain
/// Stack remains the protocol/deadline truth. These atomics only project
/// activation, explicit work, and pump admission; the attach authority lock
/// owns terminal shutdown admission.
pub(in crate::net) struct PumpControl {
    worker: spin::Once<KThreadHandle>,
    pub(super) active: AtomicBool,
    explicit_work: AtomicBool,
}

impl PumpControl {
    pub(in crate::net) fn new() -> Self {
        Self {
            worker: spin::Once::new(),
            active: AtomicBool::new(false),
            explicit_work: AtomicBool::new(false),
        }
    }

    pub(super) fn install_worker(&self, worker: KThreadHandle) {
        assert!(
            self.worker.get().is_none(),
            "network pump worker installed twice"
        );
        self.worker.call_once(|| worker);
    }

    pub(super) fn wake_worker(&self) {
        if let Some(worker) = self.worker.get() {
            worker.wake();
        }
    }

    pub(in crate::net) fn request_work(&self) {
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

    pub(super) fn work_requested(&self) -> bool {
        self.explicit_work.load(Ordering::Acquire)
    }

    pub(super) fn take_work_request(&self) -> bool {
        self.explicit_work.swap(false, Ordering::AcqRel)
    }

    pub(in crate::net) fn request_shutdown(&self) {
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

pub(super) fn deadline_due(deadline: Option<NetworkInstant>) -> bool {
    deadline.is_some_and(|deadline| deadline <= network_now())
}

pub(super) fn schedule_deadline(control: Arc<PumpControl>, deadline: NetworkInstant) {
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

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn explicit_work_is_coalesced_and_late_requests_cannot_restore_admission() {
        let control = Arc::new(PumpControl::new());
        control.request_work();
        assert!(!control.work_requested());

        control.active.store(true, Ordering::Release);
        control.request_work();
        control.request_work();
        assert!(control.take_work_request());
        assert!(!control.take_work_request());
        control.active.store(false, Ordering::Release);
        control.request_work();
        assert!(!control.work_requested());
    }

    #[kunit]
    fn request_after_a_worker_take_survives_for_the_next_bounded_round() {
        let control = PumpControl::new();
        control.active.store(true, Ordering::Release);
        control.request_work();
        assert!(control.take_work_request());

        // Models a producer committing while the worker is already inside the
        // round admitted by the request above. The next wait predicate must
        // still observe this later obligation.
        control.request_work();
        assert!(control.take_work_request());
    }
}
