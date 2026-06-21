//! TODO: add FrameOomHandler trait
//!
//! **NOTE**
//!
//! medadata in memmap is not maintained here. the allocator only serves as an
//! algorithm to allocate and deallocate physical pages.
//!
//! metadata is maintained, instead, in [crate::mm::frame::managed] module, with
//! RAII types to ensure safety.

use crate::prelude::*;

pub trait FrameAllocator {
    /// Adds a range of physical pages to the allocator's pool of available
    /// pages.
    ///
    /// # Safety
    ///
    /// **NO OVERLAP WITH EXISTING RANGES**
    unsafe fn add_range(&mut self, range: PhysPageRange);

    /// Allocates a contiguous range of physical pages.
    ///
    /// Returns the starting physical page number of the allocated range, or
    /// `None` if allocation fails.
    fn alloc(&mut self, npages: usize) -> Option<PhysPageNum>;

    /// Deallocates a range of physical pages.
    ///
    /// This method is given a [PhysPageRange] as a parameter instead of a
    /// starting [PhysPageNum] such that the allocator implementation can
    /// get more metadata about the range being deallocated, which
    /// may be helpful.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the range being deallocated was previously
    /// allocated and has not already been deallocated. The behavior is
    /// undefined if the caller violates this requirement.
    unsafe fn dealloc(&mut self, range: PhysPageRange);

    /// Returns statistics about the frame allocator.
    ///
    /// This method is intended for debugging and testing purposes, and may be
    /// implemented in a way that is not efficient. It should not be called in
    /// performance-critical code paths.
    fn stats(&self) -> FrameAllocatorStats;
}

/// TODO: more detailed statistics, such as fragmentation, peak memory usage,
/// etc.
mod allocator_stats {
    use super::*;

    #[derive(Debug, Clone, Copy)]
    pub struct FrameAllocatorStats {
        pub total_pages: u64,
        pub free_pages: u64,
    }

    impl FrameAllocatorStats {
        pub const ZEROED: Self = Self {
            total_pages: 0,
            free_pages: 0,
        };

        pub fn used_pages(&self) -> u64 {
            self.total_pages - self.free_pages
        }

        pub fn exceeds_io_shrink_threshold(&self) -> bool {
            crate::const_assert!(
                IO_SHRINK_THRESHOLD <= 100,
                "io shrink threshold must be a percentage"
            );
            self.used_pages_exceeds_percent(IO_SHRINK_THRESHOLD)
        }

        pub fn exceeds_oom_kill_threshold(&self) -> bool {
            crate::const_assert!(
                OOM_KILL_THRESHOLD <= 100,
                "oom kill threshold must be a percentage"
            );
            self.used_pages_exceeds_percent(OOM_KILL_THRESHOLD)
        }

        fn used_pages_exceeds_percent(&self, threshold_percent: u8) -> bool {
            assert!(
                threshold_percent <= 100,
                "frame usage threshold must be a percentage"
            );
            if self.total_pages == 0 {
                return false;
            }

            self.used_pages().saturating_mul(100)
                > self.total_pages.saturating_mul(threshold_percent as u64)
        }
    }
}
pub use allocator_stats::*;

#[derive(Debug)]
pub struct LockedFrameAllocator<A: FrameAllocator> {
    allocator: NoIrqSpinLock<A>,
}

impl<A: FrameAllocator> LockedFrameAllocator<A> {
    pub fn new(allocator: A) -> Self {
        Self {
            allocator: NoIrqSpinLock::new(allocator),
        }
    }

    pub fn alloc(&self, npages: usize) -> Option<OwnedFolio> {
        let start_ppn = self.allocator.lock().alloc(npages)?;
        unsafe {
            Some(OwnedFolio::new(PhysPageRange::new(
                start_ppn,
                npages as u64,
            )))
        }
    }

    pub fn alloc_one(&self) -> Option<OwnedFrameHandle> {
        let start_ppn = self.allocator.lock().alloc(1)?;
        unsafe { Some(OwnedFrameHandle::new(start_ppn)) }
    }

    pub unsafe fn add_range(&self, range: PhysPageRange) {
        let mut allocator = self.allocator.lock();
        unsafe {
            allocator.add_range(range);
        }
    }

    pub unsafe fn dealloc(&self, range: PhysPageRange) {
        let mut allocator = self.allocator.lock();
        unsafe {
            allocator.dealloc(range);
        }
    }

    pub fn stats(&self) -> FrameAllocatorStats {
        let allocator = self.allocator.lock();
        allocator.stats()
    }
}
