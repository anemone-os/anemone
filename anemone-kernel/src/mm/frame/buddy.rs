//! Buddy system for physical memory management.

use core::ptr::NonNull;

use crate::{
    mm::frame::allocator::{FrameAllocator, FrameAllocatorStats},
    prelude::*,
};

const MIN_BLOCK_BYTES: usize = PagingArch::PAGE_SIZE_BYTES;
const NORDER: usize = 11;

pub(super) struct BuddyAllocator {
    buddy: buddy_system::BuddySystem<MIN_BLOCK_BYTES, NORDER>,
}

impl BuddyAllocator {
    pub fn new() -> Self {
        Self {
            buddy: buddy_system::BuddySystem::new(),
        }
    }
}

impl FrameAllocator for BuddyAllocator {
    unsafe fn add_range(&mut self, range: PhysPageRange) {
        // the buddy system need to embed metadata inside the managed memory, so we need
        // to convert the physical page range to a writable virtual page range before
        // adding it to the buddy system.
        let range = range.to_hhdm();
        unsafe {
            let slice = core::slice::from_raw_parts_mut(
                range.start().to_virt_addr().as_ptr_mut::<u8>(),
                (range.npages() << PagingArch::PAGE_SIZE_BITS) as usize,
            );
            self.buddy
                .add_zone_from_slice(NonNull::new_unchecked(slice));
        }
    }

    fn alloc(&mut self, npages: usize) -> Option<PhysPageNum> {
        let order = (npages.next_power_of_two() as usize).trailing_zeros() as usize;
        if order > NORDER {
            return None;
        }
        self.buddy.alloc(order).ok().map(|ptr| {
            // note that the ptr returned by buddy system points to hhdm region, so we need
            // to convert it back to physical address before returning.
            let vaddr = VirtAddr::new(ptr.as_ptr() as u64);
            let paddr = unsafe { vaddr.hhdm_to_phys() };

            // It looks like that,
            // current return type PhysPageNum is not a goot choice since it
            // loses the provenance information of the allocated block.
            // We should refine this later, maybe by introducing a new type that can
            // carry the provenance information.
            PhysPageNum::new(paddr.get() >> PagingArch::PAGE_SIZE_BITS as u64)
        })
    }

    unsafe fn dealloc(&mut self, range: PhysPageRange) {
        let range = range.to_hhdm();
        unsafe {
            // we need to convert the PhysPageRange back to VirtPageRange.
            let vaddr = range.start().to_virt_addr();
            let order = (range.npages() as usize)
                .next_power_of_two()
                .trailing_zeros() as usize;
            self.buddy
                .dealloc(NonNull::new_unchecked(vaddr.as_ptr_mut::<u8>()), order)
                .unwrap_or_else(|_| panic!("failed to dealloc range: {:?}", range));
        }
    }

    fn stats(&self) -> FrameAllocatorStats {
        let mut stats = FrameAllocatorStats::ZEROED;
        for zone_stat in self.buddy.iter_zone_stats() {
            stats.total_pages += zone_stat.allocable_bytes / MIN_BLOCK_BYTES as u64;
            stats.free_pages += (zone_stat.allocable_bytes - zone_stat.cur_allocated_bytes)
                / MIN_BLOCK_BYTES as u64;
        }
        stats
    }
}
