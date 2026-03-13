#[cfg(feature = "stats")]
use crate::stats::ZoneStats;
use crate::{
    adapter::FreeBlockAdapter,
    aligned::AlignedAddr,
    bitmap::{BitSlice, BitSliceMut},
    error::BuddyError,
};
use core::{cell::RefCell, ptr::NonNull};
use intrusive_collections::{LinkedList, LinkedListLink, UnsafeRef};

#[derive(Debug)]
pub(crate) struct FreeBlock {
    pub(crate) link: LinkedListLink,
}

impl FreeBlock {
    pub(crate) const fn buddy_block_idx(block_idx: usize) -> usize {
        // note that idx is the index of block, not the index of unit
        if block_idx % 2 == 0 {
            block_idx + 1
        } else {
            block_idx - 1
        }
    }
}

/// Each zone is a contiguous region of physical memory managed by a buddy
/// allocator. The buddy system maintains a linked list of zones, and each zone
/// maintains bitmaps for each order to track the allocation status of blocks
/// within that zone.
///
/// MIN_BLOCK_BYTES are always set to PAGE_SIZE, and NORDER determines the
/// maximum block size that can be allocated (i.e., 2^(NORDER-1) *
/// MIN_BLOCK_BYTES).
///
/// For a memory region managed by Zone, the layout is as follows:
/// +-------------------+
/// | LinkedListLink    |
/// +-------------------+
/// | Zone metadata     |
/// +-------------------+
/// | Bitmap for order 0|
/// +-------------------+
/// | Bitmap for order 1|
/// +-------------------+
/// | ...               |
/// +-------------------+
/// | Bitmap for order N |
/// +-------------------+ -> salloc (start of allocable blocks)
/// | Allocable blocks   |
/// +-------------------+
///
/// TODO: explain the reason why PhantomPinned is not used here.
#[derive(Debug)]
pub(crate) struct Zone<const MIN_BLOCK_BYTES: usize, const NORDER: usize> {
    /// Looks like we place a highly dangerous self-referential pointer here.
    /// But we need this pointer to retrieve back the provenance of the whole
    /// memory region managed by Zone.
    ///
    /// This is a compromise due to Rust's memory safety model. In C there is no
    /// need for extra 16 bytes to store this pointer... This, definitely,
    /// is not a zero-cost abstraction.
    ///
    /// ⬆ Skill issue: 'Why you need this pointer? you could just use DST, which
    /// only takes 8 bytes for metadata, a half of the size of this pointer'
    ///
    /// Yes... But it's too complicated. Maybe later we can refactor the code to
    /// use DST and get rid of this pointer. For now, let's just keep it simple
    /// and straightforward.
    ///
    /// **DO NOT MODIFY ZONE THROUGH THIS FIELD**
    region: NonNull<[u8]>,

    #[cfg(feature = "stats")]
    stats: ZoneStats,

    /// Number of MIN_BLOCK_BYTES.
    ///
    /// This field is necessary since it's possible that the last byte of the
    /// bitmaps is not fully used.
    nunits: usize,

    /// Pre-calculated offsets of bitmaps for each order, in bytes.
    ///
    /// This is a small optimization to avoid calculating bitmap offsets every
    /// time we need to access the bitmaps.
    ///
    /// The offset is calculated from the start of the region.
    bitmap_offset: [usize; NORDER],

    /// Where allocable blocks start in this zone.
    salloc: AlignedAddr<MIN_BLOCK_BYTES>,

    /// Free lists for each order.
    ///
    /// These lists are mainly used to quickly find free blocks of a certain
    /// order in O(1) time, rather than scanning the bitmaps.
    free_lists: [LinkedList<FreeBlockAdapter>; NORDER],
}

#[derive(Debug)]
pub(crate) struct ZoneNode<const MIN_BLOCK_BYTES: usize, const NORDER: usize> {
    /// For single-threaded inner mutability.
    pub(crate) inner: RefCell<Zone<MIN_BLOCK_BYTES, NORDER>>,
    /// Link of BuddySystem's linked list of zones.
    pub(crate) link: LinkedListLink,
}

impl<const MIN_BLOCK_BYTES: usize, const NORDER: usize> ZoneNode<MIN_BLOCK_BYTES, NORDER> {
    pub(crate) const ZEROED: Self = Self {
        inner: RefCell::new(Zone {
            region: NonNull::slice_from_raw_parts(NonNull::dangling(), 0),
            #[cfg(feature = "stats")]
            stats: ZoneStats::ZEROED,
            nunits: 0,
            bitmap_offset: [0; NORDER],
            salloc: AlignedAddr::ZERO,
            free_lists: [const { LinkedList::new(FreeBlockAdapter::NEW) }; NORDER],
        }),
        link: LinkedListLink::new(),
    };
}

impl<const MIN_BLOCK_BYTES: usize, const NORDER: usize> ZoneNode<MIN_BLOCK_BYTES, NORDER> {
    pub(crate) unsafe fn from_slice(slice: NonNull<[u8]>) -> UnsafeRef<Self> {
        unsafe {
            let base = slice.as_ptr().cast::<u8>();
            let size = slice.len();
            let start_addr = base as usize;

            // 1. align up to align of Self and place a uninitialized header there
            let hdr_align = align_of::<Self>();
            let hdr_start = (start_addr + hdr_align - 1) & !(hdr_align - 1);
            assert!(
                hdr_start + size_of::<Self>() <= start_addr + size,
                "zone size is too small to hold metadata"
            );
            let hdr_ptr = base.with_addr(hdr_start).cast::<Self>();
            hdr_ptr.write(Self::ZEROED);

            // 2. calculate bitmap sizes and thus salloc
            // here is a chicken-and-egg problem since bitmap sizes depend on units, and
            // units depend on salloc which depends on bitmap sizes.
            //
            // to deal with this, we take a 2-pass approach:
            // - in the first pass, we overestimate size of bitmaps by overestimating units,
            //   and thus calculate salloc.
            // - in the second pass, we calculate the actual units based on salloc.
            // this way the size of bitmaps will be larger than needed, as is a
            // pessimization. it mayby inefficient, but absolutely correct.

            let overestimated_nunits = size / MIN_BLOCK_BYTES;
            let mut bitmap_bytes = 0;
            for o in 0..NORDER {
                let bits = overestimated_nunits / (1 << o);
                let bytes = crate::align_up!(bits, 8) / 8;
                bitmap_bytes += bytes;
            }
            let bitmaps_start = hdr_start + size_of::<Self>();

            // zero out bitmaps.
            assert!(
                bitmaps_start + bitmap_bytes <= start_addr + size,
                "zone size is too small to hold metadata"
            );
            let bitmaps_ptr = base.with_addr(bitmaps_start).cast::<u8>();
            bitmaps_ptr.write_bytes(0, bitmap_bytes);

            let salloc = AlignedAddr::align_up(bitmaps_start + bitmap_bytes);
            assert!(
                salloc.as_usize() < start_addr + size,
                "zone size is too small to hold metadata"
            );
            let actual_nunits = (start_addr + size - salloc.as_usize()) / MIN_BLOCK_BYTES;

            // 3. initialize the header with correct values.

            // **NOTE**
            // from the point of view of system consistency, the zone structure is not fully
            // initialized until the bitmaps, bitmap offset cache, and free lists are
            // initialized.
            //
            // however from the point of view of Rust's memory safety, the zone structure
            // has already been fully initialized after step 3 since all
            // fields are initialized and the following initialization only modifies
            // data, which are not used until the zone is fully initialized.
            //
            // That's why we can safely access bitmaps and linked list through casting
            // hdr_ptr to &mut Self (i.e. this) in below initialization code.

            let this = (&mut *hdr_ptr).inner.get_mut();
            this.region = slice;
            this.nunits = actual_nunits;
            this.salloc = salloc;

            let mut offset = bitmaps_start - start_addr;
            for o in 0..NORDER {
                this.bitmap_offset[o] = offset;
                let bits = actual_nunits / (1 << o);
                let bytes = crate::align_up!(bits, 8) / 8;
                offset += bytes;
            }

            // 4. finally, initialize the free lists and bitmaps.
            // we adopt a top-down greedy initialization strategy, i.e., we try to put as
            // many blocks of the largest order as possible, then the second largest order,
            // and so on.

            let mut rem_nunits = actual_nunits;
            let mut cur_nunits = 0;
            for o in (0..NORDER).rev() {
                let units_per_block = 1 << o;
                let nblocks = rem_nunits / units_per_block;
                rem_nunits -= nblocks * units_per_block;

                let start_idx = cur_nunits / units_per_block;
                // this assertion always holds cz we calculate from the largest order down to
                // the smallest order.
                assert!(cur_nunits % units_per_block == 0);
                for i in 0..nblocks {
                    let free_list = &mut this.free_lists[o];
                    let block_addr = this.salloc.as_usize()
                        + (cur_nunits + i * units_per_block) * MIN_BLOCK_BYTES;
                    let block_ptr = base.with_addr(block_addr).cast::<FreeBlock>();

                    block_ptr.write(FreeBlock {
                        link: LinkedListLink::new(),
                    });

                    free_list.push_back(UnsafeRef::from_raw(block_ptr));

                    let mut bitmap = this.bitmap_for_order_mut(o);
                    bitmap.set(start_idx + i).unwrap();
                }
                cur_nunits += nblocks * units_per_block;
            }

            #[cfg(feature = "stats")]
            {
                this.stats.allocable_bytes = (actual_nunits * MIN_BLOCK_BYTES) as u64;
            }

            UnsafeRef::from_raw(hdr_ptr)
        }
    }
}

impl<const MIN_BLOCK_BYTES: usize, const NORDER: usize> Zone<MIN_BLOCK_BYTES, NORDER> {
    pub(crate) fn bitmap_for_order(&self, order: usize) -> BitSlice<'_> {
        assert!(order < NORDER);
        let bits = self.nunits / (1 << order);
        unsafe {
            let offset = self.bitmap_offset[order];
            let ptr = self.region.as_ptr().cast::<u8>().wrapping_add(offset);
            BitSlice::from_raw_parts(ptr, bits)
        }
    }

    pub(crate) fn bitmap_for_order_mut(&mut self, order: usize) -> BitSliceMut<'_> {
        assert!(order < NORDER);
        let bits = self.nunits / (1 << order);
        unsafe {
            let offset = self.bitmap_offset[order];
            let ptr = self.region.as_ptr().cast::<u8>().wrapping_add(offset);
            BitSliceMut::from_raw_parts(ptr, bits)
        }
    }

    fn on_alloc(&mut self, bytes: u64) {
        #[cfg(feature = "stats")]
        {
            self.stats.total_allocations += 1;
            self.stats.cur_allocated_bytes += bytes;
            if self.stats.cur_allocated_bytes > self.stats.peak_allocated_bytes {
                self.stats.peak_allocated_bytes = self.stats.cur_allocated_bytes;
            }
        }
    }

    // nahhhh... rust's support for const generics is still too nasty to do some
    // simple compile-time calculations like this...
    // fn alloc<const ORDER: usize>(
    //     &mut self,
    // ) -> Option<AlignedAddr<{ MIN_BLOCK_BYTES * (1 << ORDER) }>> {
    //     todo!()
    // }

    pub(crate) fn alloc(&mut self, order: usize) -> Result<NonNull<u8>, BuddyError> {
        if order >= NORDER {
            return Err(BuddyError::InvalidOrder);
        }

        // 1. find a free block of the given order or higher order
        for o in order..NORDER {
            if let Some(block) = self.alloc_block(o) {
                // 2. if the block is of higher order, split it until we get a block of the
                //    desired order we always take the first half as the allocated block

                // If consistency of buddy system has been maintained,
                // all lower order blocks contained in the allocated block should be marked as
                // allocated in the bitmaps(i.e. set to 0), so we need to mark
                // buddies as free when splitting.

                let mut cur_block_idx = self.block_idx(
                    AlignedAddr::new(block.as_ptr() as usize)
                        .expect("Internal error: block address not aligned"),
                    o,
                );
                for split_order in ((order + 1)..=o).rev() {
                    let lower_block_idx = cur_block_idx * 2;
                    let lower_buddy_idx = lower_block_idx + 1;
                    let mut lower_bitmap = self.bitmap_for_order_mut(split_order - 1);
                    #[cfg(debug_assertions)]
                    {
                        assert!(
                            !lower_bitmap.test(lower_block_idx).unwrap()
                                && !lower_bitmap.test(lower_buddy_idx).unwrap(),
                            "Internal error: buddy system consistency violated"
                        );
                    }
                    lower_bitmap
                        .set(lower_buddy_idx)
                        .expect("Internal error: bitmap index out of bounds");
                    let lower_buddy_ptr = self.block_ptr(lower_buddy_idx, split_order - 1);
                    unsafe {
                        lower_buddy_ptr.cast::<FreeBlock>().write(FreeBlock {
                            link: LinkedListLink::new(),
                        });
                    }

                    let freeblock =
                        unsafe { UnsafeRef::from_raw(lower_buddy_ptr.cast::<FreeBlock>()) };
                    self.free_lists[split_order - 1].push_back(freeblock);

                    cur_block_idx = lower_block_idx;
                }

                self.on_alloc((MIN_BLOCK_BYTES * (1 << order)) as u64);
                // block and 'cur_block' should have the same address.
                return Ok(block);
            }
        }

        Err(BuddyError::OutOfMemory)
    }

    pub(crate) fn block_idx(
        &self,
        block_addr: AlignedAddr<MIN_BLOCK_BYTES>,
        order: usize,
    ) -> usize {
        let nunits_per_block = 1 << order;
        let bytes_per_block = MIN_BLOCK_BYTES * nunits_per_block;
        (block_addr.as_usize() - self.salloc.as_usize()) / bytes_per_block
    }

    pub(crate) fn block_ptr(&self, block_idx: usize, order: usize) -> *mut u8 {
        let nunits_per_block = 1 << order;
        let bytes_per_block = MIN_BLOCK_BYTES * nunits_per_block;
        let addr = self.salloc.as_usize() + block_idx * bytes_per_block;

        // Note that we must derive the pointer provenance from the original region
        // pointer to obey Rust's memory model.
        self.region.as_ptr().with_addr(addr).cast::<u8>()
    }

    fn alloc_block(&mut self, order: usize) -> Option<NonNull<u8>> {
        if order >= NORDER {
            panic!("Internal error: invalid order");
        }

        if let Some(block) = self.free_lists[order].pop_front() {
            let hdr_ptr = UnsafeRef::into_raw(block).cast::<u8>();
            unsafe {
                hdr_ptr.write_bytes(0, size_of::<FreeBlock>());
            }
            let nunits_per_block = 1 << order;
            let block_idx =
                (hdr_ptr as usize - self.salloc.as_usize()) / (MIN_BLOCK_BYTES * nunits_per_block);
            let mut bitmap = self.bitmap_for_order_mut(order);
            bitmap
                .clear(block_idx)
                .expect("Internal error: bitmap index out of bounds");
            return Some(NonNull::new(hdr_ptr).expect("Internal error: block address is null"));
        }

        None
    }

    fn on_dealloc(&mut self, bytes: u64) {
        #[cfg(feature = "stats")]
        {
            self.stats.total_deallocations += 1;
            self.stats.cur_allocated_bytes -= bytes;
        }
    }

    pub(crate) fn contains(&self, addr: usize) -> bool {
        let start = self.salloc.as_usize();
        let end = self.salloc.as_usize() + self.nunits * MIN_BLOCK_BYTES;
        addr >= start && addr < end
    }

    pub(crate) unsafe fn dealloc(
        &mut self,
        addr: NonNull<u8>,
        order: usize,
    ) -> Result<(), BuddyError> {
        if order >= NORDER {
            return Err(BuddyError::InvalidOrder);
        }

        let block_addr = AlignedAddr::<MIN_BLOCK_BYTES>::new(addr.as_ptr() as usize)
            .ok_or(BuddyError::UnalignedAddr)?;
        let block_idx = self.block_idx(block_addr, order);

        let mut cur_block_idx = block_idx;
        let mut cur_order = order;
        for o in order..NORDER {
            cur_order = o;
            if o == NORDER - 1 {
                break;
            }
            let buddy_idx = FreeBlock::buddy_block_idx(cur_block_idx);
            let mut bitmap = self.bitmap_for_order_mut(o);
            if buddy_idx >= bitmap.len()
                || !bitmap
                    .test(buddy_idx)
                    .expect("Internal error: bitmap index out of bounds")
            {
                // buddy is allocated or does not exist, cannot merge, stop here.
                break;
            }
            bitmap
                .clear(buddy_idx)
                .expect("Internal error: bitmap index out of bounds");
            let buddy_ptr = self.block_ptr(buddy_idx, o).cast::<FreeBlock>();

            // SAFETY: since the bitmap indicates that the buddy block is free, it must be
            // in the free list of the current order.
            unsafe {
                let mut buddy_cursor = self.free_lists[o].cursor_mut_from_ptr(buddy_ptr);
                let freeblock = buddy_cursor
                    .remove()
                    .expect("Internal error: buddy block not found in free list");
                let freeblock_ptr = UnsafeRef::into_raw(freeblock).cast::<u8>();
                freeblock_ptr.write_bytes(0, size_of::<FreeBlock>());
            }

            cur_block_idx /= 2;
        }

        // mark the (potentially merged) block as free and add it to the free list
        let mut bitmap = self.bitmap_for_order_mut(cur_order);
        bitmap
            .set(cur_block_idx)
            .expect("Internal error: bitmap index out of bounds");
        let block_ptr = self.block_ptr(cur_block_idx, cur_order);
        unsafe {
            block_ptr.cast::<FreeBlock>().write(FreeBlock {
                link: LinkedListLink::new(),
            });
        }
        let freeblock = unsafe { UnsafeRef::from_raw(block_ptr.cast::<FreeBlock>()) };
        self.free_lists[cur_order].push_back(freeblock);

        self.on_dealloc((MIN_BLOCK_BYTES * (1 << order)) as u64);

        Ok(())
    }

    #[cfg(feature = "stats")]
    pub(crate) fn stats(&self) -> ZoneStats {
        self.stats
    }
}

impl<const MIN_BLOCK_BYTES: usize, const NORDER: usize> Drop for Zone<MIN_BLOCK_BYTES, NORDER> {
    fn drop(&mut self) {
        // panic!("Buddy allocator should be of 'static lifetime and never
        // dropped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{aligned::AlignedAddr, error::BuddyError};
    use core::ptr::NonNull;
    use intrusive_collections::UnsafeRef;

    #[test]
    fn zone_basic_alloc_dealloc() {
        let mut data = [0u8; 4096 * 10];
        unsafe {
            let zone_ptr = ZoneNode::<1024, 4>::from_slice(NonNull::new_unchecked(&mut data));
            let zone = (&mut *(UnsafeRef::into_raw(zone_ptr))).inner.get_mut();
            //let zone = &mut *(UnsafeRef::into_raw(zone_ptr) as *const _ as *mut
            // Zone<1024, 4>);

            // Allocate all possible blocks of order 0
            let mut addrs = Vec::new();
            while let Ok(addr) = zone.alloc(0) {
                addrs.push(addr);
            }

            assert!(!addrs.is_empty());
            println!("Allocated {} blocks of order 0", addrs.len());

            // Try to allocate one more, should fail
            assert!(matches!(zone.alloc(0), Err(BuddyError::OutOfMemory)));

            // Deallocate all
            for addr in addrs {
                zone.dealloc(addr, 0).expect("Dealloc failed");
            }

            // Should be able to allocate a large block now (merging check)
            let large_block = zone
                .alloc(3)
                .expect("Failed to allocate large block after dealloc");
            zone.dealloc(large_block, 3)
                .expect("Dealloc large block failed");
        }
    }

    #[test]
    fn zone_buddy_merge() {
        let mut data = [0u8; 4096 * 8];
        unsafe {
            let zone_ptr = ZoneNode::<2048, 3>::from_slice(NonNull::new_unchecked(&mut data));
            let zone = (&mut *(UnsafeRef::into_raw(zone_ptr))).inner.get_mut();

            // Allocate two adjacent blocks of order 0
            let addr1 = zone.alloc(0).expect("Alloc 1 failed");
            let addr2 = zone.alloc(0).expect("Alloc 2 failed");

            // Check if they are buddies (assuming sequential allocation)
            let idx1 = zone.block_idx(AlignedAddr::new(addr1.as_ptr() as usize).unwrap(), 0);
            let idx2 = zone.block_idx(AlignedAddr::new(addr2.as_ptr() as usize).unwrap(), 0);

            if idx1 ^ 1 == idx2 {
                println!("Blocks are buddies: {} and {}", idx1, idx2);
                zone.dealloc(addr1, 0).expect("Dealloc 1 failed");
                zone.dealloc(addr2, 0).expect("Dealloc 2 failed");

                // Now they should be merged into an order 1 block
                let addr3 = zone.alloc(1).expect("Failed to allocate merged block");
                // Check if addr3 is one of the original addresses (likely addr1)
                assert!(addr3 == addr1 || addr3 == addr2);
            }
        }
    }

    #[test]
    fn zone_oom() {
        let mut data = [0u8; 4096 * 2]; // Very small zone
        unsafe {
            let zone_ptr = ZoneNode::<2048, 2>::from_slice(NonNull::new_unchecked(&mut data));
            let zone = (&mut *(UnsafeRef::into_raw(zone_ptr))).inner.get_mut();

            let mut addrs = Vec::new();
            while let Ok(addr) = zone.alloc(0) {
                addrs.push(addr);
            }

            assert!(matches!(zone.alloc(0), Err(BuddyError::OutOfMemory)));
            assert!(matches!(zone.alloc(1), Err(BuddyError::OutOfMemory)));
        }
    }

    #[test]
    fn zone_stats_verification() {
        let mut data = [0u8; 4096 * 16];
        unsafe {
            //let zone_ptr = Zone::<1024, 5>::from_slice(NonNull::new_unchecked(&mut
            // data));
            let zone_ptr = ZoneNode::<1024, 5>::from_slice(NonNull::new_unchecked(&mut data));
            let zone = (&mut *(UnsafeRef::into_raw(zone_ptr))).inner.get_mut();

            #[cfg(feature = "stats")]
            {
                let initial_allocable = zone.stats.allocable_bytes;
                assert!(initial_allocable > 0);
                assert_eq!(zone.stats.cur_allocated_bytes, 0);
            }

            let addr1 = zone.alloc(0).expect("Alloc 1 failed");
            let addr2 = zone.alloc(2).expect("Alloc 2 failed");

            #[cfg(feature = "stats")]
            {
                assert_eq!(zone.stats.total_allocations, 2);
                assert_eq!(zone.stats.cur_allocated_bytes, 1024 + 4096);
                assert_eq!(zone.stats.peak_allocated_bytes, 1024 + 4096);
            }

            zone.dealloc(addr1, 0).expect("Dealloc 1 failed");

            #[cfg(feature = "stats")]
            {
                assert_eq!(zone.stats.total_deallocations, 1);
                assert_eq!(zone.stats.cur_allocated_bytes, 4096);
            }

            zone.dealloc(addr2, 2).expect("Dealloc 2 failed");

            #[cfg(feature = "stats")]
            {
                assert_eq!(zone.stats.cur_allocated_bytes, 0);
                assert_eq!(zone.stats.total_deallocations, 2);
            }
        }
    }

    #[test]
    fn zone_stress_and_fragmentation() {
        let mut data = [0u8; 4096 * 32];
        unsafe {
            let zone_ptr = ZoneNode::<1024, 6>::from_slice(NonNull::new_unchecked(&mut data));
            let zone = (&mut *(UnsafeRef::into_raw(zone_ptr))).inner.get_mut();

            let mut allocations = Vec::new();

            // 1. Fragment the memory by allocating small blocks
            for _ in 0..20 {
                if let Ok(addr) = zone.alloc(0) {
                    allocations.push((addr, 0));
                }
            }

            // 2. Allocate some larger blocks
            for _ in 0..5 {
                if let Ok(addr) = zone.alloc(2) {
                    allocations.push((addr, 2));
                }
            }

            #[cfg(feature = "stats")]
            let peak_before = zone.stats.cur_allocated_bytes;

            // 3. Deallocate in a different order (every second one)
            let mut i = 0;
            while i < allocations.len() {
                let (addr, order) = allocations.remove(i);
                zone.dealloc(addr, order).expect("Dealloc failed");
                if i < allocations.len() {
                    i += 1;
                }
            }

            // 4. Deallocate the rest
            while !allocations.is_empty() {
                let (addr, order) = allocations.pop().unwrap();
                zone.dealloc(addr, order).expect("Dealloc failed");
            }

            #[cfg(feature = "stats")]
            {
                assert_eq!(zone.stats.cur_allocated_bytes, 0);
                assert!(zone.stats.peak_allocated_bytes >= peak_before);
            }

            // 5. Try to allocate a very large block to ensure merging worked
            let large = zone
                .alloc(5)
                .expect("Merging failed, could not allocate order 5 block");
            zone.dealloc(large, 5).expect("Dealloc failed");
        }
    }
}
