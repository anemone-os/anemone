#[cfg(feature = "stats")]
use crate::stats::ZoneStatsIter;
use crate::{adapter::BuddyZoneAdapter, error::BuddyError, zone::ZoneNode};
use core::ptr::NonNull;
use intrusive_collections::LinkedList;

/// A buddy system allocator that can manage multiple non-contiguous memory
/// zones.
///
/// `MIN_BLOCK_BYTES` defines the size of the smallest allocatably block
/// (typically a page size) and must be a power of two.
/// `NORDER` defines the number of orders in the allocator, where the maximum
/// block size is `MIN_BLOCK_BYTES * 2^(NORDER-1)`.
#[derive(Debug)]
pub struct BuddySystem<const MIN_BLOCK_BYTES: usize, const NORDER: usize> {
    pub(crate) zones: LinkedList<BuddyZoneAdapter<MIN_BLOCK_BYTES, NORDER>>,
}

// SAFETY:
// `BuddySystem` is safe to move to another thread (`Send`) because all
// mutating operations require `&mut self`, so safe code cannot access the same
// allocator concurrently from multiple threads.
//
// Interior mutability is implemented with `RefCell` inside each `ZoneNode`,
// but those `RefCell`s are only accessed through this unique mutable borrow.
// This type is intentionally not `Sync`.
unsafe impl<const MIN_BLOCK_BYTES: usize, const NORDER: usize> Send
    for BuddySystem<MIN_BLOCK_BYTES, NORDER>
{
}

impl<const MIN_BLOCK_BYTES: usize, const NORDER: usize> BuddySystem<MIN_BLOCK_BYTES, NORDER> {
    const __VALIDATE: () = {
        assert!(
            MIN_BLOCK_BYTES.is_power_of_two(),
            "MIN_BLOCK_BYTES must be a power of two"
        );
        assert!(
            MIN_BLOCK_BYTES >= size_of::<crate::zone::FreeBlock>(),
            "MIN_BLOCK_BYTES is too small to hold metadata"
        )
    };

    /// Creates an empty `BuddySystem` allocator.
    pub const fn new() -> Self {
        let _ = Self::__VALIDATE;
        Self {
            zones: LinkedList::new(BuddyZoneAdapter::NEW),
        }
    }

    /// Adds a new memory zone to the allocator from a slice.
    ///
    /// # Safety
    ///
    /// - The provided memory region must be valid and not used by any other
    ///   part of the system for the lifetime of this allocator.
    /// - The region must be large enough to hold metadata.
    /// - The region must be not overlapping with any existing zones already
    ///   added to this allocator.
    pub unsafe fn add_zone_from_slice(&mut self, zone: NonNull<[u8]>) {
        let node = unsafe { ZoneNode::<MIN_BLOCK_BYTES, NORDER>::from_slice(zone) };
        self.zones.push_back(node);
    }

    /// Adds a new memory zone to the allocator from a fixed-size array.
    ///
    /// # Safety
    ///
    /// - The provided memory region must be valid and not used by any other
    ///   part of the system for the lifetime of this allocator.
    /// - The region must be large enough to hold metadata.
    /// - The region must be not overlapping with any existing zones already
    ///   added to this allocator.
    pub unsafe fn add_zone_from_array<const N: usize>(&mut self, zone: NonNull<[u8; N]>) {
        let node = unsafe {
            ZoneNode::<MIN_BLOCK_BYTES, NORDER>::from_slice(NonNull::new_unchecked(
                core::slice::from_raw_parts_mut(zone.as_ptr() as *mut u8, N),
            ))
        };
        self.zones.push_back(node);
    }

    /// Allocates a block of memory of the given order.
    ///
    /// Returns a pointer to the start of the block on success, or an error if
    /// allocation fails.
    pub fn alloc(&mut self, order: usize) -> Result<NonNull<u8>, BuddyError> {
        for zone in self.zones.iter() {
            match zone.inner.borrow_mut().alloc(order) {
                Ok(addr) => return Ok(addr),
                Err(BuddyError::OutOfMemory) => continue,
                Err(e) => return Err(e),
            }
        }

        Err(BuddyError::OutOfMemory)
    }

    /// Deallocates a previously allocated block of memory.
    ///
    /// # Safety
    ///
    /// - `addr` must have been previously allocated by this allocator via
    ///   `alloc`.
    /// - `order` must match the order used when the block was allocated.
    pub unsafe fn dealloc(&mut self, addr: NonNull<u8>, order: usize) -> Result<(), BuddyError> {
        // currently an O(n) search operation to find the corresponding zone.
        // we should optimize this later by doing a binary search.
        for zone in self.zones.iter() {
            let mut zone = zone.inner.borrow_mut();
            if zone.contains(addr.as_ptr() as usize) {
                return unsafe { zone.dealloc(addr, order) };
            }
        }

        Err(BuddyError::InvalidAddr)
    }

    /// Returns an iterator over the statistics of each zone.
    #[cfg(feature = "stats")]
    pub fn iter_zone_stats(&self) -> ZoneStatsIter<'_, MIN_BLOCK_BYTES, NORDER> {
        ZoneStatsIter {
            zone_iter: self.zones.iter(),
        }
    }
}

impl<const MIN_BLOCK_BYTES: usize, const NORDER: usize> Drop
    for BuddySystem<MIN_BLOCK_BYTES, NORDER>
{
    fn drop(&mut self) {
        // panic!("Buddy allocator should be of 'static lifetime and never
        // dropped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_multi_zone_alloc() {
        let mut data1 = [0u8; 4096 * 4];
        let mut data2 = [0u8; 4096 * 4];
        unsafe {
            let mut system = BuddySystem::<1024, 3>::new();
            system.add_zone_from_slice(NonNull::new_unchecked(&mut data1));
            system.add_zone_from_slice(NonNull::new_unchecked(&mut data2));

            let mut allocations = Vec::new();
            // Allocate more than what one zone can provide
            // Zone metadata takes some space, so each 16KB zone won't have 16 order-0
            // blocks.
            while let Ok(addr) = system.alloc(0) {
                allocations.push(addr);
            }

            assert!(
                allocations.len() > 4,
                "Should have allocated plenty of blocks"
            );

            // Deallocate all
            for addr in allocations {
                system.dealloc(addr, 0).expect("System dealloc failed");
            }

            // Should be able to allocate a large block now
            let large = system
                .alloc(2)
                .expect("Should be able to allocate order 2 block after dealloc");
            system.dealloc(large, 2).expect("Dealloc large failed");
        }
    }

    #[test]
    fn system_invalid_dealloc() {
        let mut data = [0u8; 4096 * 4];
        unsafe {
            let mut system = BuddySystem::<1024, 3>::new();
            system.add_zone_from_slice(NonNull::new_unchecked(&mut data));

            let addr = NonNull::new_unchecked(0xdeadbeef as *mut u8);
            assert_eq!(system.dealloc(addr, 0), Err(BuddyError::InvalidAddr));
        }
    }

    #[test]
    fn system_stats_iter() {
        let mut data1 = [0u8; 4096 * 8];
        let mut data2 = [0u8; 4096 * 8];
        unsafe {
            let mut system = BuddySystem::<1024, 4>::new();
            system.add_zone_from_slice(NonNull::new_unchecked(&mut data1));
            system.add_zone_from_slice(NonNull::new_unchecked(&mut data2));

            #[cfg(feature = "stats")]
            {
                let stats: Vec<_> = system.iter_zone_stats().collect();
                assert_eq!(stats.len(), 2);
                assert_eq!(stats[0].total_allocations, 0);
                assert_eq!(stats[1].total_allocations, 0);

                // Allocate from first zone
                let addr1 = system.alloc(0).unwrap();
                let stats_after = system.iter_zone_stats().collect::<Vec<_>>();
                assert_eq!(stats_after[0].total_allocations, 1);
                assert_eq!(stats_after[1].total_allocations, 0);

                // Drain first zone and start allocating from second
                let mut addrs = vec![addr1];
                while let Ok(addr) = system.alloc(0) {
                    addrs.push(addr);
                }

                let final_stats = system.iter_zone_stats().collect::<Vec<_>>();
                assert!(final_stats[0].total_allocations > 0);
                assert!(final_stats[1].total_allocations > 0);

                for addr in addrs {
                    system.dealloc(addr, 0).unwrap();
                }
            }
        }
    }

    #[test]
    fn system_add_zone_from_array() {
        unsafe {
            let mut data = [0u8; 4096 * 4];
            let mut system = BuddySystem::<1024, 3>::new();
            system.add_zone_from_array(NonNull::new_unchecked(&mut data));

            let addr = system.alloc(0).expect("Alloc from array zone failed");
            system.dealloc(addr, 0).unwrap();
        }
    }

    #[test]
    fn fuzz_crash_repro() {
        let mut regions = [[0u8; 8192]; 4];
        {
            let mut system = BuddySystem::<16, 5>::new();
            for region in &mut regions {
                unsafe {
                    system.add_zone_from_slice(core::ptr::NonNull::new_unchecked(region));
                }
            }

            // [
            //     Alloc {
            //         order: 4,
            //     },
            // ]

            let _ptr = system.alloc(4).expect("Alloc order 4 failed");
        }
    }
}
