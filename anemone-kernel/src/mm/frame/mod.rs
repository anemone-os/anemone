// Physical frame management.

use bitflags::bitflags;
use spin::Lazy;

use crate::{
    mm::frame::{allocator::LockedFrameAllocator, buddy::BuddyAllocator},
    prelude::*,
};

pub(super) mod allocator;
mod buddy;
mod managed;
pub use managed::*;

#[derive(Debug, Clone, Copy)]
pub struct AvailMemZone {
    start_ppn: PhysPageNum,
    npages: u64,
}

impl AvailMemZone {
    pub fn new(start_ppn: PhysPageNum, npages: u64) -> Self {
        Self { start_ppn, npages }
    }

    pub fn range(&self) -> PhysPageRange {
        PhysPageRange::new(self.start_ppn, self.npages)
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct RsvMemFlags: u32 {
        /// Memory that should not be mapped by the kernel's paging
        /// subsystem.
        const NOMAP = 0x0001;

        /// Memory that can be reused by kernel.
        const REUSABLE = 0x0002;

        /// Kernel image region.
        const KVIRT = 0x0004;

        /// Memory that can be used for early allocation before
        /// the frame allocator is initialized.
        const EARLY_ALLOC = 0x0008;
    }
}

impl RsvMemFlags {
    pub fn is_mappable(&self) -> bool {
        !self.contains(RsvMemFlags::NOMAP)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RsvMemZone {
    start_ppn: PhysPageNum,
    npages: u64,
    flags: RsvMemFlags,
}

impl RsvMemZone {
    pub fn new(start_ppn: PhysPageNum, npages: u64, flags: RsvMemFlags) -> Self {
        Self {
            start_ppn,
            npages,
            flags,
        }
    }

    pub fn range(&self) -> PhysPageRange {
        PhysPageRange::new(self.start_ppn, self.npages)
    }

    pub fn flags(&self) -> RsvMemFlags {
        self.flags
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MemZone {
    Avail(AvailMemZone),
    Rsv(RsvMemZone),
}

// TODO: switch to a more appropriate synchronization primitive.
// currently SpinLock is used for simplicity.

// LOCK ORDERING: MEM_ZONES -> AVAIL_MEM_ZONES -> RSV_MEM_ZONES

pub static MEM_ZONES: Lazy<SpinLock<Vec<MemZone>>> = Lazy::new(|| SpinLock::new(Vec::new()));
pub static AVAIL_MEM_ZONES: Lazy<SpinLock<Vec<AvailMemZone>>> =
    Lazy::new(|| SpinLock::new(Vec::new()));
pub static RSV_MEM_ZONES: Lazy<SpinLock<Vec<RsvMemZone>>> = Lazy::new(|| SpinLock::new(Vec::new()));

/// Adds a memory zone to the physical memory manager.
///
/// # Safety
/// The caller must ensure that the memory zone specified by `zone` is valid
/// and **does not overlap** with any existing memory zones. The
/// behavior is undefined if the caller violates this requirement.
pub unsafe fn add_mem_zone(zone: MemZone) {
    let mut mem_zones = MEM_ZONES.lock_irqsave();
    let mut avail_mem_zones = AVAIL_MEM_ZONES.lock_irqsave();
    let mut rsv_mem_zones = RSV_MEM_ZONES.lock_irqsave();

    kinfoln!("add_mem_zone: adding memory zone: {:x?}", zone);
    mem_zones.push(zone);
    match zone {
        MemZone::Avail(avail_zone) => avail_mem_zones.push(avail_zone),
        MemZone::Rsv(rsv_zone) => rsv_mem_zones.push(rsv_zone),
    }
}

static FRAME_ALLOCATOR: Lazy<LockedFrameAllocator<BuddyAllocator>> =
    Lazy::new(|| LockedFrameAllocator::new(BuddyAllocator::new()));

/// Initializes the physical memory manager.
///
/// # Safety
///
/// This function must be called exactly once during kernel initialization,
/// after all memory zones have been added via [`add_mem_zone`]. The behavior is
/// undefined if this function is called multiple times or if it is called
/// before all memory zones have been added.
pub unsafe fn pmm_init() {
    let avail_mem_zones = AVAIL_MEM_ZONES.lock_irqsave();
    for zone in avail_mem_zones.iter() {
        let range = PhysPageRange::new(zone.start_ppn, zone.npages);
        unsafe {
            FRAME_ALLOCATOR.add_range(range);
        }
    }
}

pub fn frame_allocator_stats() -> allocator::FrameAllocatorStats {
    FRAME_ALLOCATOR.stats()
}

mod alloc {
    use super::*;

    /// Allocates a contiguous range of physical pages.
    pub fn alloc_frames(npages: usize) -> Option<Folio> {
        FRAME_ALLOCATOR.alloc(npages)
    }

    /// Allocates a single physical page.
    pub fn alloc_frame() -> Option<Frame> {
        FRAME_ALLOCATOR.alloc_one()
    }

    /// Allocates a contiguous range of physical pages and zeroes them.
    pub fn alloc_frames_zeroed(npages: usize) -> Option<Folio> {
        let mut folio = alloc_frames(npages)?;
        folio.as_bytes_mut().fill(0);
        Some(folio)
    }

    /// Allocates a single physical page and zeroes it.
    pub fn alloc_frame_zeroed() -> Option<Frame> {
        let mut frame = alloc_frame()?;
        frame.as_bytes_mut().fill(0);
        Some(frame)
    }
}
pub use alloc::*;
