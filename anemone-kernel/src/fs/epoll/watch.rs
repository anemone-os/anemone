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

    fn route_interests(self) -> PollEvent {
        // Epoll policy stays consumer-owned. Subscribe to every readiness class
        // so a source that only supports one class (for example timerfd's
        // READABLE route) can still carry later ERROR/HANG_UP recheck hints;
        // delivery is filtered separately by the user policy below.
        PollEvent::READABLE | PollEvent::WRITABLE | PollEvent::ERROR | PollEvent::HANG_UP
    }

    fn delivery_interests(self) -> PollEvent {
        self.interests | PollEvent::ERROR | PollEvent::HANG_UP
    }

    pub(super) fn deliverable(self, snapshot: PollEvent) -> PollEvent {
        snapshot & self.delivery_interests()
    }

    pub(super) const fn edge_triggered(self) -> bool {
        self.edge_triggered
    }

    pub(super) const fn one_shot(self) -> bool {
        self.one_shot
    }

    pub(super) const fn user_data(self) -> u64 {
        self.user_data
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
    /// Sticky ET recheck obligation for this immutable watch generation.
    /// It is protocol state, not a cached target-readiness snapshot.
    dirty: AtomicBool,
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
            // ADD and successful MOD must snapshot the new generation once
            // even when the source publishes no concurrent transition.
            dirty: AtomicBool::new(true),
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

    pub(super) const fn policy(&self) -> WatchPolicy {
        self.policy
    }

    pub(super) fn subscribe(
        self: &Arc<Self>,
        lease: &OpenedDescriptionLease,
    ) -> Result<PollEvent, SysError> {
        let observer: Arc<dyn PollObserver> = self.clone();
        let route = PollRoute::new(&observer);
        drop(observer);
        let request = PollRequest::register_with_route(self.policy.route_interests(), &route);

        match lease.poll(&request)? {
            PollRegisterResult::Subscribed(current) => Ok(current),
            // The route is installed, but only a later snapshot can classify
            // readiness. Initial dirty publication makes that recheck part of
            // the ADD/MOD operation instead of sleeping on an unknown state.
            PollRegisterResult::SubscribedRecheck => Ok(PollEvent::empty()),
            PollRegisterResult::Ready(_) | PollRegisterResult::Unsupported => {
                Err(SysError::PermissionDenied)
            },
        }
    }

    pub(super) fn snapshot(&self, lease: &OpenedDescriptionLease) -> Result<PollEvent, SysError> {
        let request = PollRequest::snapshot(self.policy.route_interests());
        match lease.poll(&request)? {
            PollRegisterResult::Ready(current) => Ok(current),
            unexpected => {
                kwarningln!(
                    "epoll: target snapshot returned unexpected result {:?}",
                    unexpected,
                );
                Err(SysError::IO)
            },
        }
    }

    pub(super) fn claim_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    pub(super) fn restore_dirty(&self) {
        self.dirty.store(true, Ordering::Release);
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

        // Retirement may race after the acceptance check. Dirty belongs to
        // this immutable watch generation, so a late callback cannot create
        // ET delivery for a replacement that reuses the same slot.
        self.restore_dirty();
        owner.publish_wait_activity();
    }
}
