use crate::{
    prelude::*,
    task::files::{Fd, OpenedDescriptionCapability},
};

mod file;
mod ready;
mod watch;

use file::EpollFileRoutes;
use ready::{DirtySlots, ReadySlots, SLOT_COUNT, SlotId, WatchSlots};
use watch::EpollWatch;
pub(in crate::fs) use watch::WatchPolicy;

struct EpollOperation {
    closing: bool,
    slots: WatchSlots,
    ready: ReadySlots,
}

/// Owner of one epoll instance's watch and ready protocols.
///
/// The mutex serializes ctl/teardown/refresh/harvest decisions; it does not own
/// target readiness or opened-description liveness. Source callbacks only
/// publish sticky dirty bits and an epoll-file notification sequence.
pub(in crate::fs) struct Epoll {
    operation: Mutex<EpollOperation>,
    dirty: DirtySlots,
    notification_sequence: AtomicU64,
    file_routes: EpollFileRoutes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fs) struct EpollEvent {
    events: PollEvent,
    user_data: u64,
}

impl EpollEvent {
    pub(in crate::fs) const fn events(self) -> PollEvent {
        self.events
    }

    pub(in crate::fs) const fn user_data(self) -> u64 {
        self.user_data
    }
}

struct HarvestClaim {
    slot: SlotId,
    watch: Arc<EpollWatch>,
}

/// Operation-serialized candidate batch awaiting syscall copyout policy.
///
/// Dropping an uncommitted batch restores every claimed candidate. The future
/// syscall adapter may therefore validate/copy bytes without turning partial
/// user-memory progress into an event or ONESHOT policy commit.
pub(in crate::fs) struct EpollHarvest<'a> {
    operation: MutexGuard<'a, EpollOperation>,
    claims: Vec<HarvestClaim>,
    events: Vec<EpollEvent>,
    committed: bool,
}

impl EpollHarvest<'_> {
    pub(in crate::fs) fn events(&self) -> &[EpollEvent] {
        &self.events
    }

    pub(in crate::fs) fn commit(mut self) {
        assert!(!self.committed, "epoll harvest batch committed twice");
        assert_eq!(
            self.claims.len(),
            self.events.len(),
            "epoll harvest claim/event count diverged"
        );

        for claim in &self.claims {
            let current = self
                .operation
                .slots
                .current(claim.slot)
                .expect("epoll harvest commit lost current watch");
            assert!(
                Arc::ptr_eq(&current, &claim.watch),
                "epoll harvest commit crossed watch generation"
            );

            let policy = claim.watch.policy();
            if policy.one_shot() {
                self.operation.ready.disable(claim.slot);
            } else if !policy.edge_triggered() {
                // The pre-copyout snapshot remains a candidate hint only. A
                // concurrent not-ready transition publishes dirty and the next
                // operation rechecks it before reporting epoll-file readiness.
                self.operation.ready.make_candidate(claim.slot);
            }
        }
        self.committed = true;
    }
}

impl Drop for EpollHarvest<'_> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }

        for claim in &self.claims {
            let current = self
                .operation
                .slots
                .current(claim.slot)
                .expect("epoll harvest rollback lost current watch");
            assert!(
                Arc::ptr_eq(&current, &claim.watch),
                "epoll harvest rollback crossed watch generation"
            );
            assert!(
                !self.operation.ready.is_disabled(claim.slot),
                "uncommitted epoll harvest disabled ONESHOT"
            );
            self.operation.ready.make_candidate(claim.slot);
        }
    }
}

impl Epoll {
    pub(in crate::fs) fn try_new() -> Result<Arc<Self>, SysError> {
        let slots = WatchSlots::try_new()?;
        let file_routes = EpollFileRoutes::try_new()?;
        Arc::try_new(Self {
            operation: Mutex::new(EpollOperation {
                closing: false,
                slots,
                ready: ReadySlots::new(),
            }),
            dirty: DirtySlots::new(),
            notification_sequence: AtomicU64::new(0),
            file_routes,
        })
        .map_err(|_| SysError::OutOfMemory)
    }

    pub(in crate::fs) fn ctl_add(
        self: &Arc<Self>,
        fd: Fd,
        target: OpenedDescriptionCapability,
        policy: WatchPolicy,
    ) -> Result<(), SysError> {
        let mut operation = self.operation.lock();
        Self::ensure_open(&operation)?;
        Self::prune_retired(&mut operation);
        if operation.slots.find_current(&target, fd).is_some() {
            return Err(SysError::AlreadyExists);
        }

        let lease = target.try_lease().ok_or(SysError::BadFileDescriptor)?;
        let reservation = operation.slots.reserve()?;
        let watch = match EpollWatch::try_new(
            Arc::downgrade(self),
            reservation.slot(),
            reservation.generation(),
            fd,
            target,
            policy,
        ) {
            Ok(watch) => watch,
            Err(err) => {
                operation.slots.release_reserved(reservation);
                return Err(err);
            },
        };

        if let Err(err) = watch.subscribe(&lease) {
            assert!(watch.retire());
            operation.slots.release_reserved(reservation);
            return Err(err);
        }
        // This final acquire load is the ADD linearization point against final
        // close. A close racing after it is ordered after this publication and
        // will make the new watch stale for the next refresh.
        if !lease.is_live() {
            assert!(watch.retire());
            operation.slots.release_reserved(reservation);
            return Err(SysError::BadFileDescriptor);
        }
        if operation.slots.find_current(watch.target(), fd).is_some() {
            assert!(watch.retire());
            operation.slots.release_reserved(reservation);
            return Err(SysError::AlreadyExists);
        }

        let slot = reservation.slot();
        operation.ready.reset(slot);
        operation.slots.publish_reserved(reservation, watch);
        drop(operation);
        self.note_dirty(slot);
        Ok(())
    }

    pub(in crate::fs) fn ctl_modify(
        self: &Arc<Self>,
        fd: Fd,
        target: OpenedDescriptionCapability,
        policy: WatchPolicy,
    ) -> Result<(), SysError> {
        let mut operation = self.operation.lock();
        Self::ensure_open(&operation)?;
        let (slot, current) = operation
            .slots
            .find_current(&target, fd)
            .ok_or(SysError::NotFound)?;
        let lease = target.try_lease().ok_or(SysError::BadFileDescriptor)?;
        let generation = operation.slots.next_replacement_generation(slot)?;
        let replacement =
            EpollWatch::try_new(Arc::downgrade(self), slot, generation, fd, target, policy)?;

        if let Err(err) = replacement.subscribe(&lease) {
            assert!(replacement.retire());
            return Err(err);
        }
        // As in ADD, this load orders the replacement commit against terminal
        // close without caching a second alive bit in epoll.
        if !lease.is_live() {
            assert!(replacement.retire());
            return Err(SysError::BadFileDescriptor);
        }

        let still_current = operation
            .slots
            .find_current(replacement.target(), fd)
            .expect("epoll MOD lost current key while holding operation mutex");
        assert_eq!(still_current.0, slot);
        assert!(Arc::ptr_eq(&still_current.1, &current));
        let retired = operation.slots.replace_current(slot, replacement);
        operation.ready.reset(slot);
        assert!(Arc::ptr_eq(&retired, &current));
        assert!(retired.retire(), "epoll MOD replaced a retired watch");
        drop(operation);
        self.note_dirty(slot);
        Ok(())
    }

    pub(in crate::fs) fn ctl_delete(
        &self,
        fd: Fd,
        target: &OpenedDescriptionCapability,
    ) -> Result<(), SysError> {
        let mut operation = self.operation.lock();
        Self::ensure_open(&operation)?;
        let (slot, current) = operation
            .slots
            .find_current(target, fd)
            .ok_or(SysError::NotFound)?;
        let retired = operation.slots.remove_current(slot);
        operation.ready.reset(slot);
        assert!(Arc::ptr_eq(&retired, &current));
        assert!(retired.retire(), "epoll DEL removed a retired watch");
        Ok(())
    }

    /// Build a candidate batch while retaining the per-instance operation
    /// permit for the future syscall adapter's all-or-rollback copyout.
    pub(in crate::fs) fn harvest(&self, maxevents: usize) -> Result<EpollHarvest<'_>, SysError> {
        if maxevents == 0 {
            return Err(SysError::InvalidArgument);
        }
        let limit = core::cmp::min(maxevents, SLOT_COUNT);
        let mut claims = Vec::new();
        claims
            .try_reserve_exact(limit)
            .map_err(|_| SysError::OutOfMemory)?;
        let mut events = Vec::new();
        events
            .try_reserve_exact(limit)
            .map_err(|_| SysError::OutOfMemory)?;

        let operation = self.operation.lock();
        Self::ensure_open(&operation)?;
        let mut batch = EpollHarvest {
            operation,
            claims,
            events,
            committed: false,
        };
        self.refresh_dirty(&mut batch.operation)?;

        for _ in 0..SLOT_COUNT {
            if batch.events.len() == limit {
                break;
            }
            let Some(slot) = batch.operation.ready.claim_next() else {
                break;
            };
            let watch = batch
                .operation
                .slots
                .current(slot)
                .expect("epoll ready candidate has no current watch");
            let Some(lease) = watch.target().try_lease() else {
                Self::retire_current(&mut batch.operation, slot, &watch);
                continue;
            };
            let snapshot = match watch.snapshot(&lease) {
                Ok(snapshot) => snapshot,
                Err(err) => {
                    self.dirty.mark(slot);
                    return Err(err);
                },
            };
            if !lease.is_live() {
                Self::retire_current(&mut batch.operation, slot, &watch);
                continue;
            }
            let deliverable = watch.policy().deliverable(snapshot);
            if deliverable.is_empty() {
                continue;
            }

            batch.claims.push(HarvestClaim {
                slot,
                watch: watch.clone(),
            });
            batch.events.push(EpollEvent {
                events: deliverable,
                user_data: watch.policy().user_data(),
            });
        }

        Ok(batch)
    }

    /// Permanently close this instance and retire every current watch.
    ///
    /// The epoll opened-description final-release hook is the normal caller.
    /// Watched targets are never entered or synchronously unlinked.
    pub(in crate::fs) fn teardown(&self) {
        let mut operation = self.operation.lock();
        if operation.closing {
            return;
        }
        operation.closing = true;
        operation.slots.retire_all();
        operation.ready.reset_all();
        drop(operation);
        self.publish_file_hint();
    }

    fn refresh_dirty(&self, operation: &mut EpollOperation) -> Result<(), SysError> {
        // Final target close deliberately does not enter epoll or publish a
        // wake. Operations therefore perform this bounded liveness sweep so
        // disabled/quiet watches cannot consume all slots until teardown.
        Self::prune_retired(operation);
        let dirty = self.dirty.take();
        let mut first_error = None;
        for slot in dirty.slots() {
            if operation.ready.is_disabled(slot) {
                continue;
            }
            if let Err(err) = self.refresh_slot(operation, slot)
                && first_error.is_none()
            {
                first_error = Some(err);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    fn refresh_slot(&self, operation: &mut EpollOperation, slot: SlotId) -> Result<(), SysError> {
        let Some(watch) = operation.slots.current(slot) else {
            operation.ready.reset(slot);
            return Ok(());
        };
        let Some(lease) = watch.target().try_lease() else {
            Self::retire_current(operation, slot, &watch);
            return Ok(());
        };
        let snapshot = match watch.snapshot(&lease) {
            Ok(snapshot) => snapshot,
            Err(err) => {
                // The target did not produce a stable snapshot. Keep the slot
                // dirty so the error does not silently consume the obligation.
                self.dirty.mark(slot);
                return Err(err);
            },
        };
        if !lease.is_live() {
            Self::retire_current(operation, slot, &watch);
            return Ok(());
        }

        if watch.policy().deliverable(snapshot).is_empty() {
            operation.ready.consume_candidate(slot);
        } else {
            operation.ready.make_candidate(slot);
        }
        Ok(())
    }

    fn has_deliverable_candidate(&self, operation: &mut EpollOperation) -> Result<bool, SysError> {
        for index in 0..SLOT_COUNT {
            let slot = SlotId::from_index(index);
            if !operation.ready.is_candidate(slot) {
                continue;
            }
            let watch = operation
                .slots
                .current(slot)
                .expect("epoll ready candidate has no current watch");
            let Some(lease) = watch.target().try_lease() else {
                Self::retire_current(operation, slot, &watch);
                continue;
            };
            let snapshot = match watch.snapshot(&lease) {
                Ok(snapshot) => snapshot,
                Err(err) => {
                    self.dirty.mark(slot);
                    return Err(err);
                },
            };
            if !lease.is_live() {
                Self::retire_current(operation, slot, &watch);
                continue;
            }
            if watch.policy().deliverable(snapshot).is_empty() {
                operation.ready.consume_candidate(slot);
                continue;
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn retire_current(operation: &mut EpollOperation, slot: SlotId, expected: &Arc<EpollWatch>) {
        let current = operation
            .slots
            .current(slot)
            .expect("epoll retirement lost current watch");
        assert!(
            Arc::ptr_eq(&current, expected),
            "epoll retirement crossed watch generation"
        );
        let retired = operation.slots.remove_current(slot);
        operation.ready.reset(slot);
        assert!(Arc::ptr_eq(&retired, expected));
        assert!(
            retired.retire(),
            "epoll retired a non-accepting current watch"
        );
    }

    fn prune_retired(operation: &mut EpollOperation) {
        for index in 0..SLOT_COUNT {
            let slot = SlotId::from_index(index);
            let Some(watch) = operation.slots.current(slot) else {
                continue;
            };
            if watch.target().try_lease().is_none() {
                Self::retire_current(operation, slot, &watch);
            }
        }
    }

    fn poll_file(&self, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
        if !request.is_register() {
            let mut operation = self.operation.lock();
            Self::ensure_open(&operation)?;
            self.refresh_dirty(&mut operation)?;
            let readable = self.has_deliverable_candidate(&mut operation)?;
            let events = if readable {
                PollEvent::READABLE & request.interests()
            } else {
                PollEvent::empty()
            };
            return Ok(PollRegisterResult::Ready(events));
        }

        if !request.interests().contains(PollEvent::READABLE) {
            return Ok(PollRegisterResult::Unsupported);
        }
        let route = request
            .route()
            .expect("epoll-file register request lost its route");
        let before = self.notification_sequence.load(Ordering::Acquire);
        let mut operation = self.operation.lock();
        Self::ensure_open(&operation)?;
        self.refresh_dirty(&mut operation)?;
        let mut readable = self.has_deliverable_candidate(&mut operation)?;
        self.file_routes.subscribe(route)?;

        // A callback between the first sequence sample and route publication
        // either changes this value and is absorbed below, or occurs after
        // publication and also notifies the installed route.
        let after = self.notification_sequence.load(Ordering::Acquire);
        if after != before {
            self.refresh_dirty(&mut operation)?;
            readable = self.has_deliverable_candidate(&mut operation)?;
        }
        let current = if readable {
            PollEvent::READABLE
        } else {
            PollEvent::empty()
        };
        Ok(PollRegisterResult::Subscribed(current))
    }

    fn ensure_open(operation: &EpollOperation) -> Result<(), SysError> {
        if operation.closing {
            Err(SysError::IdentifierRemoved)
        } else {
            Ok(())
        }
    }

    fn note_dirty(&self, slot: SlotId) {
        self.dirty.mark(slot);
        self.publish_file_hint();
    }

    fn publish_file_hint(&self) {
        let mut observed = self.notification_sequence.load(Ordering::Relaxed);
        loop {
            let next = observed
                .checked_add(1)
                .expect("epoll notification sequence exhausted");
            match self.notification_sequence.compare_exchange_weak(
                observed,
                next,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(current) => observed = current,
            }
        }
        self.file_routes.notify();
    }
}
