use crate::{
    prelude::*,
    task::files::{Fd, OpenedDescriptionCapability},
};

mod ready;
mod watch;

use ready::{DirtySlots, SlotId, WatchSlots};
use watch::EpollWatch;
pub(in crate::fs) use watch::WatchPolicy;

struct EpollOperation {
    closing: bool,
    slots: WatchSlots,
}

/// Owner of one epoll instance's watch lifecycle and serialized operations.
///
/// The mutex serializes ctl/teardown decisions; it does not own target
/// readiness or opened-description liveness. Source callbacks only publish to
/// `dirty`, which is a candidate obligation and is revalidated by generation.
pub(in crate::fs) struct Epoll {
    operation: Mutex<EpollOperation>,
    dirty: DirtySlots,
}

impl Epoll {
    pub(in crate::fs) fn try_new() -> Result<Arc<Self>, SysError> {
        let slots = WatchSlots::try_new()?;
        Arc::try_new(Self {
            operation: Mutex::new(EpollOperation {
                closing: false,
                slots,
            }),
            dirty: DirtySlots::new(),
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
        assert!(Arc::ptr_eq(&retired, &current));
        assert!(retired.retire(), "epoll DEL removed a retired watch");
        Ok(())
    }

    /// Permanently close this instance and retire every current watch.
    ///
    /// The future epoll opened-description final-release hook is the only
    /// normal caller. Watched targets are never entered or synchronously
    /// unlinked.
    pub(in crate::fs) fn teardown(&self) {
        let mut operation = self.operation.lock();
        if operation.closing {
            return;
        }
        operation.closing = true;
        operation.slots.retire_all();
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
    }
}
