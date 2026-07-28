use crate::{
    prelude::*,
    task::files::{Fd, OpenedDescriptionCapability},
};

mod file;
mod ready;
mod watch;

use file::EpollWaitPublication;
pub(in crate::fs) use file::{create_epoll_file, epoll_from_file, teardown_epoll_file};
use ready::{SLOT_COUNT, ScanSlots, SlotId, WatchSlots};
use watch::EpollWatch;
pub(in crate::fs) use watch::WatchPolicy;

struct EpollOperation {
    closing: bool,
    slots: WatchSlots,
    scan: ScanSlots,
}

/// Owner of one epoll instance's watch and ready protocols.
///
/// The mutex serializes ctl/teardown/scan/harvest decisions; it does not own
/// target readiness or opened-description liveness. Source callbacks only
/// publish per-watch ET causality and invalidate the epoll-file wait coverage.
pub(in crate::fs) struct Epoll {
    operation: Mutex<EpollOperation>,
    wait_publication: EpollWaitPublication,
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
    edge_claimed: bool,
}

/// Operation-serialized event batch awaiting syscall copyout policy.
///
/// Dropping an uncommitted batch restores every claimed ET obligation. The
/// syscall adapter may therefore validate/copy bytes without turning partial
/// user-memory progress into an event or ONESHOT policy commit.
pub(in crate::fs) struct EpollHarvest<'a> {
    epoll: &'a Epoll,
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
                self.operation.scan.disable(claim.slot);
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
                !self.operation.scan.is_disabled(claim.slot),
                "uncommitted epoll harvest disabled ONESHOT"
            );
            if claim.edge_claimed {
                claim.watch.restore_dirty();
            }
        }
        if !self.claims.is_empty() {
            // Copyout failure keeps every event and ONESHOT policy unconsumed.
            // Re-publish activity so already-armed peer waiters cannot remain
            // asleep after this operation releases the table permit.
            self.epoll.publish_wait_activity();
        }
    }
}

impl Epoll {
    pub(in crate::fs) fn try_new() -> Result<Arc<Self>, SysError> {
        let slots = WatchSlots::try_new()?;
        let wait_publication = EpollWaitPublication::try_new()?;
        Arc::try_new(Self {
            operation: Mutex::new(EpollOperation {
                closing: false,
                slots,
                scan: ScanSlots::new(),
            }),
            wait_publication,
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
        operation.scan.reset(slot);
        operation.slots.publish_reserved(reservation, watch);
        // Mapping publication precedes coverage invalidation while the
        // operation owner still excludes scans. Notifications run outside the
        // publication spinlock, in the permitted operation -> publication
        // lock direction.
        self.publish_wait_activity();
        drop(operation);
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
        operation.scan.reset(slot);
        assert!(Arc::ptr_eq(&retired, &current));
        assert!(retired.retire(), "epoll MOD replaced a retired watch");
        self.publish_wait_activity();
        drop(operation);
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
        operation.scan.reset(slot);
        assert!(Arc::ptr_eq(&retired, &current));
        assert!(retired.retire(), "epoll DEL removed a retired watch");
        Ok(())
    }

    /// Build a bounded-scan batch while retaining the per-instance operation
    /// permit for the syscall adapter's all-or-rollback copyout.
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

        let mut operation = self.operation.lock();
        Self::ensure_open(&operation)?;
        // Final target close deliberately does not enter epoll or publish a
        // wake. Every exact scan therefore includes a bounded liveness sweep.
        Self::prune_retired(&mut operation);
        self.wait_publication.begin_check();
        let mut batch = EpollHarvest {
            epoll: self,
            operation,
            claims,
            events,
            committed: false,
        };
        let start = batch.operation.scan.cursor();
        let mut scanned = 0;

        for offset in 0..SLOT_COUNT {
            if batch.events.len() == limit {
                break;
            }
            let slot = SlotId::from_index((start + offset) % SLOT_COUNT);
            scanned += 1;
            if batch.operation.scan.is_disabled(slot) {
                continue;
            }
            let Some(watch) = batch.operation.slots.current(slot) else {
                continue;
            };
            let edge_claimed = if watch.policy().edge_triggered() {
                if !watch.claim_dirty() {
                    continue;
                }
                true
            } else {
                false
            };
            let Some(lease) = watch.target().try_lease() else {
                Self::retire_current(&mut batch.operation, slot, &watch);
                continue;
            };
            let snapshot = match watch.snapshot(&lease) {
                Ok(snapshot) => snapshot,
                Err(err) => {
                    if edge_claimed {
                        watch.restore_dirty();
                    }
                    self.wait_publication.finish_active_check();
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
                edge_claimed,
            });
            batch.events.push(EpollEvent {
                events: deliverable,
                user_data: watch.policy().user_data(),
            });
            batch.operation.scan.advance_after(slot);
        }

        if batch.events.is_empty() && scanned == SLOT_COUNT {
            self.wait_publication.finish_empty_check();
        } else {
            // A ready result or maxevents truncation cannot certify that the
            // complete watch table was empty.
            self.wait_publication.finish_active_check();
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
        operation.scan.reset_all();
        self.wait_publication.close();
        drop(operation);
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
        operation.scan.reset(slot);
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
            let readable = self.scan_file_readable()?;
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
        // Register never enters the sleepable operation domain. EmptyCovered
        // was established by the preceding exact snapshot; otherwise the
        // publication owner returns SubscribedRecheck and self-hints.
        self.wait_publication.subscribe(route)
    }

    fn scan_file_readable(&self) -> Result<bool, SysError> {
        let mut operation = self.operation.lock();
        Self::ensure_open(&operation)?;
        Self::prune_retired(&mut operation);
        self.wait_publication.begin_check();

        for index in 0..SLOT_COUNT {
            let slot = SlotId::from_index(index);
            if operation.scan.is_disabled(slot) {
                continue;
            }
            let Some(watch) = operation.slots.current(slot) else {
                continue;
            };
            let edge_claimed = if watch.policy().edge_triggered() {
                if !watch.claim_dirty() {
                    continue;
                }
                true
            } else {
                false
            };
            let Some(lease) = watch.target().try_lease() else {
                Self::retire_current(&mut operation, slot, &watch);
                continue;
            };
            let snapshot = match watch.snapshot(&lease) {
                Ok(snapshot) => snapshot,
                Err(err) => {
                    if edge_claimed {
                        watch.restore_dirty();
                    }
                    self.wait_publication.finish_active_check();
                    return Err(err);
                },
            };
            if !lease.is_live() {
                Self::retire_current(&mut operation, slot, &watch);
                continue;
            }
            if watch.policy().deliverable(snapshot).is_empty() {
                continue;
            }

            if edge_claimed {
                // poll(epfd) is a non-consuming readability probe. Only an
                // epoll_wait copyout commit may consume ET causality.
                watch.restore_dirty();
            }
            self.wait_publication.finish_active_check();
            return Ok(true);
        }

        self.wait_publication.finish_empty_check();
        Ok(false)
    }

    fn ensure_open(operation: &EpollOperation) -> Result<(), SysError> {
        if operation.closing {
            Err(SysError::IdentifierRemoved)
        } else {
            Ok(())
        }
    }

    pub(super) fn publish_wait_activity(&self) {
        self.wait_publication.invalidate_and_notify();
    }
}
