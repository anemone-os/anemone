//! Deferred remote TLB invalidation and retired-frame cleanup.

use super::{vmo::RetiredFrames, *};

/// Prevents synchronous remote TLB invalidation while the user-space mutex is
/// held. Dropping the guard completes the invalidation before releasing any
/// retired physical frames it carries.
#[derive(Debug)]
pub struct RemoteUspFenceGuard {
    /// `None` requests one full remote flush. A range is sent as one broadcast
    /// round and acknowledged only after each CPU applies its local range
    /// policy.
    range: Option<VirtPageRange>,
    retired: RetiredFrames,
}

impl RemoteUspFenceGuard {
    pub(super) fn new(range: Option<VirtPageRange>) -> Self {
        Self {
            range,
            retired: RetiredFrames::default(),
        }
    }

    pub(super) fn with_retired(range: Option<VirtPageRange>, retired: RetiredFrames) -> Self {
        Self { range, retired }
    }
}

// Equality describes only the invalidation scope. Retired frames are a linear
// cleanup capability and deliberately do not participate in comparisons.
impl PartialEq for RemoteUspFenceGuard {
    fn eq(&self, other: &Self) -> bool {
        self.range == other.range
    }
}

impl Eq for RemoteUspFenceGuard {}

impl Drop for RemoteUspFenceGuard {
    fn drop(&mut self) {
        let fence_failed =
            if let Err(e) = broadcast_ipi(IpiPayload::TlbShootdown { range: self.range }) {
                kalertln!("failed to broadcast user TLB shootdown IPI: {e:?}");
                true
            } else {
                false
            };

        if fence_failed && !self.retired.is_empty() {
            let npages = self.retired.len();
            let retired = core::mem::take(&mut self.retired);
            // Fail-close bridge: Drop cannot return an IPI transport failure. Keep
            // the old frames out of the allocator until shootdown completion has
            // an infallible or retryable owner API, then remove this leak fallback.
            core::mem::forget(retired);
            kalertln!("retaining {npages} decommitted user frames after failed TLB shootdown");
        }
    }
}
