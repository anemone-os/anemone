// Physical frame management.

use crate::{
    mm::frame::{allocator::LockedFrameAllocator, buddy::BuddyAllocator},
    prelude::*,
};

pub(super) mod allocator;
mod buddy;
mod managed;
pub use managed::*;

mod memmap;
pub use memmap::{get_frame_raw, init as memmap_init};

static FRAME_ALLOCATOR: Lazy<LockedFrameAllocator<BuddyAllocator>> =
    Lazy::new(|| LockedFrameAllocator::new(BuddyAllocator::new()));

/// Initializes the physical memory manager.
///
/// # Safety
///
/// This function must be called exactly once during kernel initialization,
/// after all memory zones have been added. The behavior is undefined if this
/// function is called multiple times or if it is called before all memory zones
/// have been added.
pub unsafe fn pmm_init() {
    sys_mem_zones().with_avail_zones(|avail_zones| {
        for zone in avail_zones.iter() {
            let range = zone.range();
            kdebugln!("pmm_init: adding range {:?}", range);
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
pub fn alloc_frames(npages: usize) -> Option<OwnedFolio> {
    assert_ne!(npages, 0, "Internal error: cannot allocate zero pages");

    let stats = frame_allocator_stats();

    kdebugln!(
        "alloc_frames: requesting {} pages ({} bytes), stats before allocation: {:?}",
        npages,
        npages * PagingArch::PAGE_SIZE_BYTES as usize,
        stats
    );

    FRAME_ALLOCATOR.alloc(npages)
}

/// Allocates a single physical page.
pub fn alloc_frame() -> Option<OwnedFrameHandle> {
    FRAME_ALLOCATOR.alloc_one()
}

/// Allocates a contiguous range of physical pages and zeroes them.
pub fn alloc_frames_zeroed(npages: usize) -> Option<OwnedFolio> {
    let mut folio = alloc_frames(npages)?;
    folio.as_bytes_mut().fill(0);
    Some(folio)
}

/// Allocates a single physical page and zeroes it.
pub fn alloc_frame_zeroed() -> Option<OwnedFrameHandle> {
    let mut frame = alloc_frame()?;
    frame.as_bytes_mut().fill(0);
    Some(frame)
}

#[kunit]
fn alloc_frame_updates_stats_and_refcount() {
    let before = frame_allocator_stats();

    let frame = alloc_frame().expect("alloc_frame() should succeed during kunit");
    let ppn = frame.leak();

    assert_eq!(unsafe { get_frame_raw(ppn) }.rc(), 1);

    let during = frame_allocator_stats();
    assert_eq!(during.used_pages(), before.used_pages() + 1);

    let frame = unsafe { OwnedFrameHandle::from_ppn(ppn) };
    drop(frame);

    let after = frame_allocator_stats();
    assert_eq!(after.used_pages(), before.used_pages());
    assert_eq!(unsafe { get_frame_raw(ppn) }.rc(), 0);
}

#[kunit]
fn alloc_frames_updates_stats_and_refcount() {
    const NPAGES: usize = 4;

    let before = frame_allocator_stats();

    let folio = alloc_frames(NPAGES).expect("alloc_frames() should succeed during kunit");
    let range = folio.leak();

    assert_eq!(unsafe { get_frame_raw(range.start()) }.rc(), 1);

    let during = frame_allocator_stats();
    assert_eq!(during.used_pages(), before.used_pages() + NPAGES as u64);

    let folio = unsafe { OwnedFolio::from_range(range) };
    drop(folio);

    let after = frame_allocator_stats();
    assert_eq!(after.used_pages(), before.used_pages());

    assert_eq!(unsafe { get_frame_raw(range.start()) }.rc(), 0);
}
