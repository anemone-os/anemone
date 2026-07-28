use crate::prelude::*;

/// Consumer-owned endpoint behind a source-facing poll route.
///
/// This trait stays private to `fs`. Sources receive only `PollRoute`, so they
/// cannot inspect consumer state or branch on notification results.
pub(in crate::fs) trait PollObserver: Send + Sync {
    fn notify(&self);
}

/// Non-owning, source-neutral readiness notification route.
///
/// A route is only a recheck hint. It carries neither readiness truth nor a
/// consumer lifetime, and late notification after consumer retirement is
/// expected to fail closed.
#[derive(Clone)]
pub(crate) struct PollRoute {
    observer: Weak<dyn PollObserver>,
}

impl PollRoute {
    pub(in crate::fs) fn new(observer: &Arc<dyn PollObserver>) -> Self {
        Self {
            observer: Arc::downgrade(observer),
        }
    }

    /// Publish a no-return recheck hint outside the source lock.
    pub(crate) fn notify(&self) {
        if let Some(observer) = self.observer.upgrade() {
            observer.notify();
        }
    }

    /// Resource-hygiene hint only; correctness never depends on pruning.
    pub(crate) fn is_prunable(&self) -> bool {
        // Do not upgrade under a source lock: a temporary strong reference
        // could become the final observer owner and move consumer teardown
        // into the source critical section. Zero strong references are
        // terminal, so this conservative snapshot is sufficient for hygiene.
        self.observer.strong_count() == 0
    }
}

impl core::fmt::Debug for PollRoute {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PollRoute").finish_non_exhaustive()
    }
}
