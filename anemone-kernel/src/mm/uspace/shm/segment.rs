use crate::{mm::uspace::vma::Protection, prelude::*};
use anemone_abi::process::linux::shm::IpcPerm;

use super::{
    SHM_DEST, ShmObject,
    registry::{ShmId, ShmKey, ShmSeq, ShmSlotIndex},
};

/// A single attachment of one SysV shared-memory segment in an address space.
///
/// The attach address is page-granular by construction. Syscall entry points
/// may accept byte addresses, but the user-space bookkeeping should not
/// preserve the redundant byte offset once validation and rounding are
/// complete.
#[derive(Debug, Clone)]
pub struct ShmAttachment {
    pub segment: Arc<ShmSegment>,
    pub start: VirtPageNum,
    pub prot: Protection,
}

impl ShmAttachment {
    pub fn range(&self) -> VirtPageRange {
        VirtPageRange::new(self.start, self.segment.npages() as u64)
    }
}

/// Reservation for a `shmat` that is in progress.
///
/// The attach count is incremented while the segment is still protected by the
/// registry lock. This prevents a concurrent `IPC_RMID` from reclaiming the
/// slot before the VMA installation reaches the address-space lock.
pub(super) struct ShmAttachReservation {
    segment: Arc<ShmSegment>,
}

impl ShmAttachReservation {
    pub(super) fn try_new(segment: Arc<ShmSegment>) -> Result<Self, SysError> {
        let mut inner = segment.inner.lock();
        if inner.state.removed {
            return Err(SysError::IdentifierRemoved);
        }
        inner.state.attach_count = inner
            .state
            .attach_count
            .checked_add(1)
            .expect("SysV shm attach count overflow");
        drop(inner);
        Ok(Self { segment })
    }

    pub(super) fn segment(&self) -> &Arc<ShmSegment> {
        &self.segment
    }

    pub(super) fn commit(self, tgid: Tid) -> Arc<ShmSegment> {
        self.segment.record_attach(tgid);
        self.segment
    }

    pub(super) fn cancel(self) -> Arc<ShmSegment> {
        self.segment.cancel_attach_reservation();
        self.segment
    }
}

/// Kernel object for one SysV shared-memory segment.
///
/// Immutable identity and sizing data are kept lock-free after construction.
/// Mutable IPC metadata is protected by a short spin critical section because
/// it is small, non-sleeping state that may be touched from process teardown
/// paths.
#[derive(Debug)]
pub struct ShmSegment {
    id: ShmId,
    index: ShmSlotIndex,
    seq: ShmSeq,
    key: Option<ShmKey>,
    size: usize,
    inner: SpinLock<ShmSegmentInner>,
    object: Arc<ShmObject>,
}

#[derive(Debug, Clone, Copy)]
pub struct ShmSegmentState {
    /// Set after IPC_RMID. The slot is reclaimed only after the attach count
    /// reaches zero.
    pub removed: bool,
    /// Number of live address-space attachments.
    pub attach_count: usize,
    /// TGID of the creating process.
    pub creator_tgid: Tid,
    /// TGID of the last attach/detach caller.
    pub last_operator_tgid: Tid,
    /// Last successful shmat time, or zero before the first attach.
    pub last_attach_time: Duration,
    /// Last successful shmdt time, or zero before the first detach.
    pub last_detach_time: Duration,
    /// Creation time or last IPC_SET time.
    pub last_change_time: Duration,
}

#[derive(Debug, Clone, Copy)]
struct ShmSegmentInner {
    perm: IpcPerm,
    state: ShmSegmentState,
}

impl ShmSegment {
    pub(super) fn new(
        id: ShmId,
        index: ShmSlotIndex,
        seq: ShmSeq,
        key: Option<ShmKey>,
        size: usize,
        perm: IpcPerm,
        creator_tgid: Tid,
    ) -> Self {
        let npages = align_up!(size, PagingArch::PAGE_SIZE_BYTES) / PagingArch::PAGE_SIZE_BYTES;
        Self {
            id,
            index,
            seq,
            key,
            size,
            inner: SpinLock::new(ShmSegmentInner {
                perm,
                state: ShmSegmentState {
                    removed: false,
                    attach_count: 0,
                    creator_tgid,
                    last_operator_tgid: creator_tgid,
                    last_attach_time: Duration::ZERO,
                    last_detach_time: Duration::ZERO,
                    last_change_time: Instant::now().to_duration(),
                },
            }),
            object: Arc::new(ShmObject::new(npages)),
        }
    }

    pub(super) fn id(&self) -> ShmId {
        self.id
    }

    pub(super) fn index(&self) -> ShmSlotIndex {
        self.index
    }

    pub(super) fn seq(&self) -> ShmSeq {
        self.seq
    }

    pub(super) fn key(&self) -> Option<ShmKey> {
        self.key
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn npages(&self) -> usize {
        align_up!(self.size, PagingArch::PAGE_SIZE_BYTES) / PagingArch::PAGE_SIZE_BYTES
    }

    pub fn object(&self) -> Arc<ShmObject> {
        self.object.clone()
    }

    pub fn perm(&self) -> IpcPerm {
        self.inner.lock().perm
    }

    pub fn state(&self) -> ShmSegmentState {
        self.inner.lock().state
    }

    pub fn nattch(&self) -> usize {
        self.inner.lock().state.attach_count
    }

    pub fn is_removed(&self) -> bool {
        self.inner.lock().state.removed
    }

    fn increment_attach_count(&self) -> usize {
        let mut inner = self.inner.lock();
        inner.state.attach_count = inner
            .state
            .attach_count
            .checked_add(1)
            .expect("SysV shm attach count overflow");
        inner.state.attach_count
    }

    /// Account for an attachment inherited by `fork`.
    ///
    /// SysV attach timestamps describe explicit `shmat`/`shmdt` activity, so
    /// cloning an existing address-space attachment only raises `nattch`.
    pub(in crate::mm::uspace) fn inherit_attachment_for_fork(&self) -> usize {
        self.increment_attach_count()
    }

    fn record_attach(&self, tgid: Tid) -> usize {
        let now = Instant::now().to_duration();
        let mut inner = self.inner.lock();
        inner.state.last_attach_time = now;
        inner.state.last_operator_tgid = tgid;
        inner.state.attach_count
    }

    fn cancel_attach_reservation(&self) -> usize {
        let mut inner = self.inner.lock();
        assert!(
            inner.state.attach_count > 0,
            "cancelled SysV shm attach reservation must be counted"
        );
        inner.state.attach_count -= 1;
        inner.state.attach_count
    }

    pub fn on_detach(&self, tgid: Tid) -> usize {
        let now = Instant::now().to_duration();
        let mut inner = self.inner.lock();
        assert!(
            inner.state.attach_count > 0,
            "SysV shm detach must have a live attachment"
        );
        inner.state.attach_count -= 1;
        inner.state.last_detach_time = now;
        inner.state.last_operator_tgid = tgid;
        inner.state.attach_count
    }

    pub fn mark_removed(&self) -> bool {
        let mut inner = self.inner.lock();
        if inner.state.removed {
            return false;
        }
        inner.state.removed = true;
        inner.perm.mode |= SHM_DEST;
        true
    }

    pub fn update_from_ipc_set(&self, new_perm: IpcPerm) {
        let now = Instant::now().to_duration();
        let mut inner = self.inner.lock();

        inner.perm.uid = new_perm.uid;
        inner.perm.gid = new_perm.gid;
        inner.perm.mode = (inner.perm.mode & !0o777) | (new_perm.mode & 0o777);
        inner.state.last_change_time = now;
    }

    pub fn is_reclaimable(&self) -> bool {
        let state = self.inner.lock().state;
        state.removed && state.attach_count == 0
    }
}
