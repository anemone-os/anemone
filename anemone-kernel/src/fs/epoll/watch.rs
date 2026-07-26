use crate::{
    fs::iomux::{PollObserver, PollRoute},
    prelude::*,
    task::files::{Fd, OpenedDescriptionCapability, OpenedDescriptionLease},
};

use super::{
    Epoll,
    ready::{SlotGeneration, SlotId},
};

/// Internal watch policy. Linux-shaped bits remain in the future syscall
/// adapter; this type carries only epoll-owner semantics.
#[derive(Debug, Clone, Copy)]
pub(in crate::fs) struct WatchPolicy {
    interests: PollEvent,
    edge_triggered: bool,
    one_shot: bool,
    user_data: u64,
}

impl WatchPolicy {
    pub(in crate::fs) const fn new(
        interests: PollEvent,
        edge_triggered: bool,
        one_shot: bool,
        user_data: u64,
    ) -> Self {
        Self {
            interests,
            edge_triggered,
            one_shot,
            user_data,
        }
    }
}

pub(super) struct EpollWatch {
    owner: Weak<Epoll>,
    slot: SlotId,
    generation: SlotGeneration,
    fd: Fd,
    target: OpenedDescriptionCapability,
    policy: WatchPolicy,
    /// Sole watch-side truth for whether late source callbacks may publish a
    /// recheck obligation. It never represents target readiness or liveness.
    accepting: AtomicBool,
}

impl EpollWatch {
    pub(super) fn try_new(
        owner: Weak<Epoll>,
        slot: SlotId,
        generation: SlotGeneration,
        fd: Fd,
        target: OpenedDescriptionCapability,
        policy: WatchPolicy,
    ) -> Result<Arc<Self>, SysError> {
        Arc::try_new(Self {
            owner,
            slot,
            generation,
            fd,
            target,
            policy,
            accepting: AtomicBool::new(true),
        })
        .map_err(|_| SysError::OutOfMemory)
    }

    pub(super) const fn slot(&self) -> SlotId {
        self.slot
    }

    pub(super) const fn generation(&self) -> SlotGeneration {
        self.generation
    }

    pub(super) fn same_key(&self, target: &OpenedDescriptionCapability, fd: Fd) -> bool {
        self.fd == fd && self.target.same_identity(target)
    }

    pub(super) fn target(&self) -> &OpenedDescriptionCapability {
        &self.target
    }

    pub(super) fn subscribe(
        self: &Arc<Self>,
        lease: &OpenedDescriptionLease,
    ) -> Result<PollEvent, SysError> {
        let observer: Arc<dyn PollObserver> = self.clone();
        let route = PollRoute::new(&observer);
        drop(observer);
        let request = PollRequest::register_with_route(self.policy.interests, &route);

        match lease.poll(&request)? {
            PollRegisterResult::Subscribed(current) => Ok(current),
            PollRegisterResult::Ready(_) | PollRegisterResult::Unsupported => {
                Err(SysError::PermissionDenied)
            },
        }
    }

    /// Returns whether this call performed the only live-to-retired transition.
    pub(super) fn retire(&self) -> bool {
        self.accepting.swap(false, Ordering::AcqRel)
    }
}

impl PollObserver for EpollWatch {
    fn notify(&self) {
        if !self.accepting.load(Ordering::Acquire) {
            return;
        }
        let Some(owner) = self.owner.upgrade() else {
            return;
        };

        // Retirement may race after the acceptance check. Such a callback may
        // only dirty this immutable slot identity; the operation owner later
        // rejects an empty/reused slot by generation before behavior or copyout.
        owner.note_dirty(self.slot);
    }
}
