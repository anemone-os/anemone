// Physical frame management.

use spin::Lazy;

use crate::{
    mm::frame::{allocator::LockedFrameAllocator, buddy::BuddyAllocator},
    prelude::*,
};

pub(super) mod allocator;
mod buddy;
mod managed;
pub use managed::*;

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
    sys_mem_zones().with_avail_zones(|avail_zones| {
        for zone in avail_zones.iter() {
            let range = zone.range();
            unsafe {
                FRAME_ALLOCATOR.add_range(range);
            }
        }
    });
}

pub fn frame_allocator_stats() -> allocator::FrameAllocatorStats {
    FRAME_ALLOCATOR.stats()
}

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
