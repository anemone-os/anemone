use crate::{
    prelude::*,
    syscall::handler::{TryFromSyscallArg, syscall_arg_flag32},
};

use anemone_abi::process::linux::ipc::IPC_PRIVATE;

use super::{
    SHMALL, SHMMNI,
    segment::{ShmAttachReservation, ShmPerm, ShmSegment},
};

const SHM_INDEX_BITS: usize = 16;
const SHM_SEQ_BITS: usize = 15;
const SHM_INDEX_LIMIT: usize = 1usize << SHM_INDEX_BITS;
const SHM_SEQ_LIMIT: u16 = 1u16 << SHM_SEQ_BITS;
const SHM_INDEX_MASK: i32 = (SHM_INDEX_LIMIT as i32) - 1;
const SHM_SEQ_MASK: i32 = (1i32 << SHM_SEQ_BITS) - 1;

/// A real SysV shm key that participates in keyed registry lookup.
///
/// `IPC_PRIVATE` is handled at the syscall boundary and is intentionally not a
/// valid value here because it does not name a reusable key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct ShmKey(i32);

impl ShmKey {
    pub fn new(raw: i32) -> Result<Self, SysError> {
        if raw == IPC_PRIVATE {
            return Err(SysError::InvalidArgument);
        }
        Ok(Self(raw))
    }

    pub fn raw(self) -> i32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub(super) struct ShmSlotIndex(u16);

impl ShmSlotIndex {
    fn try_from_usize(index: usize) -> Result<Self, SysError> {
        if index < SHM_INDEX_LIMIT {
            Ok(Self(index as u16))
        } else {
            Err(SysError::NoSpace)
        }
    }

    pub fn from_linux_stat_target(target: i32) -> Result<Self, SysError> {
        if target < 0 {
            return Err(SysError::InvalidArgument);
        }
        Ok(Self((target & SHM_INDEX_MASK) as u16))
    }

    pub fn get(self) -> usize {
        self.0 as usize
    }

    fn raw_bits(self) -> i32 {
        self.0 as i32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub(super) struct ShmSeq(u16);

impl ShmSeq {
    const ZERO: Self = Self(0);

    fn next(self) -> Self {
        Self(self.0.wrapping_add(1) & (SHM_SEQ_LIMIT - 1))
    }

    fn raw_bits(self) -> i32 {
        self.0 as i32
    }

    pub fn raw(self) -> u16 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub(super) struct ShmId(i32);

impl ShmId {
    pub fn from_raw(raw: i32) -> Result<Self, SysError> {
        if raw < 0 {
            return Err(SysError::InvalidArgument);
        }
        Ok(Self(raw))
    }

    pub fn raw(self) -> i32 {
        self.0
    }

    fn new(index: ShmSlotIndex, seq: ShmSeq) -> Self {
        Self((seq.raw_bits() << SHM_INDEX_BITS) | index.raw_bits())
    }

    fn decode(self) -> (ShmSlotIndex, ShmSeq) {
        let index = (self.0 & SHM_INDEX_MASK) as u16;
        let seq = ((self.0 >> SHM_INDEX_BITS) & SHM_SEQ_MASK) as u16;
        (ShmSlotIndex(index), ShmSeq(seq))
    }
}

impl TryFromSyscallArg for ShmId {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        Self::from_raw(syscall_arg_flag32(raw)? as i32)
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ShmRegistryStats {
    pub used_ids: usize,
    pub allocated_pages: usize,
    pub resident_pages: usize,
    pub highest_index: Option<ShmSlotIndex>,
}

#[derive(Debug, Clone)]
struct ShmSlot {
    seq: ShmSeq,
    segment: Option<Arc<ShmSegment>>,
}

impl ShmSlot {
    fn new() -> Self {
        Self {
            seq: ShmSeq::ZERO,
            segment: None,
        }
    }
}

#[derive(Debug)]
pub(super) struct ShmRegistry {
    slots: Vec<ShmSlot>,
    free: Vec<ShmSlotIndex>,
    by_key: BTreeMap<ShmKey, ShmSlotIndex>,
    used_pages: usize,
}

// Registry operations may allocate slot vectors, key maps, and segment objects.
// Keep this as a sleeping mutex instead of a spin lock; per-segment metadata is
// separately protected by a short spin lock.
pub(super) static SYSV_SHM: Lazy<Mutex<ShmRegistry>> = Lazy::new(|| Mutex::new(ShmRegistry::new()));

pub(super) fn with_registry<R>(f: impl FnOnce(&mut ShmRegistry) -> R) -> R {
    let mut guard = SYSV_SHM.lock();
    f(&mut guard)
}

impl ShmRegistry {
    fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            by_key: BTreeMap::new(),
            used_pages: 0,
        }
    }

    fn occupied_count(&self) -> usize {
        self.slots.len() - self.free.len()
    }

    fn slot(&self, index: ShmSlotIndex) -> Option<&ShmSlot> {
        self.slots.get(index.get())
    }

    fn slot_mut(&mut self, index: ShmSlotIndex) -> Option<&mut ShmSlot> {
        self.slots.get_mut(index.get())
    }

    fn alloc_slot(&mut self) -> Result<ShmSlotIndex, SysError> {
        if self.occupied_count() >= SHMMNI {
            return Err(SysError::NoSpace);
        }

        if let Some(index) = self.free.pop() {
            assert!(self.slots[index.get()].segment.is_none());
            Ok(index)
        } else {
            let raw_index = self.slots.len();
            if raw_index >= SHMMNI {
                return Err(SysError::NoSpace);
            }
            let index = ShmSlotIndex::try_from_usize(raw_index)?;
            self.slots.push(ShmSlot::new());
            Ok(index)
        }
    }

    fn release_slot(&mut self, index: ShmSlotIndex) {
        let npages = {
            let slot = self
                .slot_mut(index)
                .expect("release slot index must be valid");
            let segment = slot
                .segment
                .take()
                .expect("release slot must contain a segment");
            slot.seq = slot.seq.next();
            segment.npages()
        };

        self.used_pages = self
            .used_pages
            .checked_sub(npages)
            .expect("used pages underflow");
        self.free.push(index);
    }

    pub fn lookup_by_key(&self, key: ShmKey) -> Option<Arc<ShmSegment>> {
        let index = *self.by_key.get(&key)?;
        self.slot(index)
            .and_then(|slot| slot.segment.as_ref())
            .cloned()
    }

    pub fn lookup_by_shmid(&self, id: ShmId) -> Result<Arc<ShmSegment>, SysError> {
        let (index, seq) = id.decode();
        let slot = self.slot(index).ok_or(SysError::InvalidArgument)?;
        if slot.seq != seq {
            return Err(SysError::InvalidArgument);
        }
        slot.segment.clone().ok_or(SysError::InvalidArgument)
    }

    pub fn reserve_attach_by_shmid(&self, id: ShmId) -> Result<ShmAttachReservation, SysError> {
        let segment = self.lookup_by_shmid(id)?;
        ShmAttachReservation::try_new(segment)
    }

    pub fn lookup_by_index(&self, index: ShmSlotIndex) -> Result<Arc<ShmSegment>, SysError> {
        self.slot(index)
            .and_then(|slot| slot.segment.as_ref())
            .cloned()
            .ok_or(SysError::InvalidArgument)
    }

    pub fn create_segment(
        &mut self,
        key: Option<ShmKey>,
        size: usize,
        perm: ShmPerm,
        creator_tgid: Tid,
    ) -> Result<Arc<ShmSegment>, SysError> {
        let npages = align_up!(size, PagingArch::PAGE_SIZE_BYTES) / PagingArch::PAGE_SIZE_BYTES;
        if npages == 0 || size == 0 {
            return Err(SysError::InvalidArgument);
        }
        if self
            .used_pages
            .checked_add(npages)
            .ok_or(SysError::NoSpace)?
            > SHMALL
        {
            return Err(SysError::NoSpace);
        }

        let index = self.alloc_slot()?;
        let seq = self.slots[index.get()].seq;
        let id = ShmId::new(index, seq);
        let mut perm = perm;
        perm.seq = seq.raw();
        let segment = Arc::new(ShmSegment::new(
            id,
            index,
            seq,
            key,
            size,
            perm,
            creator_tgid,
        ));

        self.slots[index.get()].segment = Some(segment.clone());
        if let Some(key) = key {
            self.by_key.insert(key, index);
        }
        self.used_pages += npages;

        Ok(segment)
    }

    pub fn remove_by_shmid(&mut self, id: ShmId) -> Result<Arc<ShmSegment>, SysError> {
        let (index, seq) = id.decode();
        let slot = self.slot_mut(index).ok_or(SysError::InvalidArgument)?;
        if slot.seq != seq {
            return Err(SysError::InvalidArgument);
        }

        let segment = slot
            .segment
            .as_ref()
            .ok_or(SysError::InvalidArgument)?
            .clone();
        if !segment.mark_removed() {
            return Err(SysError::IdentifierRemoved);
        }

        if let Some(key) = segment.key() {
            self.by_key.remove(&key);
        }
        if segment.is_reclaimable() {
            self.release(segment.clone());
        }

        Ok(segment)
    }

    /// Release a slot whose segment is known to be reclaimable.
    ///
    /// Callers must check `segment.is_reclaimable()` first. Violating that
    /// contract is a kernel bug and should fail loudly instead of silently
    /// hiding lifecycle mistakes.
    pub fn release(&mut self, segment: Arc<ShmSegment>) {
        assert!(segment.is_reclaimable());
        let index = segment.index();
        let slot = self
            .slot(index)
            .expect("reclaimable segment slot must exist");
        assert_eq!(slot.seq, segment.seq());
        let current = slot
            .segment
            .as_ref()
            .expect("reclaimable segment slot must be occupied");
        assert!(Arc::ptr_eq(current, &segment));

        self.release_slot(index);
    }

    pub fn stats(&self) -> ShmRegistryStats {
        let resident_pages = self
            .slots
            .iter()
            .filter_map(|slot| slot.segment.as_ref())
            .map(|segment| segment.object().resident_pages())
            .sum();
        let highest_index = self
            .slots
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, slot)| {
                slot.segment
                    .is_some()
                    .then(|| ShmSlotIndex::try_from_usize(index).ok())
                    .flatten()
            });

        ShmRegistryStats {
            used_ids: self.occupied_count(),
            allocated_pages: self.used_pages,
            resident_pages,
            highest_index,
        }
    }
}
