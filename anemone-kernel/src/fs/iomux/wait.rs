use crate::prelude::*;

use super::{
    PollRequest,
    subscription::{PollObserver, PollRoute},
};

struct IomuxWaitObserver {
    /// Sole consumer-side truth for whether source callbacks are accepted.
    /// Latch state remains scheduler-owned and is not mirrored here.
    accepting: AtomicBool,
    trigger: LatchTrigger,
}

impl IomuxWaitObserver {
    /// Returns whether this call performed the only live-to-retired transition.
    fn retire(&self) -> bool {
        self.accepting.swap(false, Ordering::AcqRel)
    }
}

impl PollObserver for IomuxWaitObserver {
    fn notify(&self) {
        // A callback that observed acceptance before retirement may race past
        // finish; LatchTrigger's wait identity makes that late trigger stale.
        if self.accepting.load(Ordering::Acquire) {
            self.trigger.trigger();
        }
    }
}

/// Linear owner of one iomux register/sleep/final-scan round.
///
/// The source-facing route is non-owning. This owner retains callback
/// acceptance and the latch until `finish`, which always retires acceptance
/// before retiring the wait identity.
pub(crate) struct IomuxWaitRound {
    latch: Option<Latch>,
    observer: Arc<IomuxWaitObserver>,
    route: PollRoute,
}

impl IomuxWaitRound {
    pub(crate) fn begin_current() -> Self {
        let latch = Latch::begin_current(true);
        let observer = Arc::new(IomuxWaitObserver {
            accepting: AtomicBool::new(true),
            trigger: latch.make_trigger(),
        });
        let erased: Arc<dyn PollObserver> = observer.clone();
        let route = PollRoute::new(&erased);
        drop(erased);

        Self {
            latch: Some(latch),
            observer,
            route,
        }
    }

    pub(crate) fn poll_request(&self, interests: super::PollEvent) -> PollRequest<'_> {
        PollRequest::register_with_route(interests, &self.route, &self.observer.trigger)
    }

    pub(in crate::fs) fn wait_id(&self) -> usize {
        self.observer.trigger.wait_id()
    }

    pub(crate) fn cancel(&self, reason: LatchCancelReason) {
        self.latch
            .as_ref()
            .expect("iomux wait round cancel after finish")
            .cancel(reason);
    }

    pub(crate) fn schedule_with_timeout(&self, timeout: Option<Duration>) -> Duration {
        self.latch
            .as_ref()
            .expect("iomux wait round schedule after finish")
            .schedule_with_timeout(timeout)
    }

    pub(crate) fn finish(mut self) -> LatchWaitOutcome {
        let retired = self.observer.retire();
        let latch = self.latch.take().expect("iomux wait round double finish");
        let outcome = latch.finish();
        assert!(retired, "iomux observer retired before wait-round finish");
        outcome
    }
}

impl Drop for IomuxWaitRound {
    fn drop(&mut self) {
        let Some(latch) = self.latch.take() else {
            return;
        };

        // Missing explicit finish is a bug, but first close both consumer
        // acceptance and the underlying wait so panic cannot leak either.
        let retired = self.observer.retire();
        latch.cancel(LatchCancelReason::Drop);
        let outcome = latch.finish();
        kwarningln!(
            "iomux: wait round dropped without finish wait={:#x} outcome={:?}",
            self.observer.trigger.wait_id(),
            outcome,
        );
        assert!(retired, "iomux observer retired before wait-round drop");
        assert!(false, "iomux wait round dropped without finish");
    }
}
