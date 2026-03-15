//! Memmap, tracking metadata of all physical frames.
//!
//! PPN is split into two parts: the lower part is the in-section idx, and
//! the higher part are indirect levels. Each indirect level has
//! BITS_PER_INDIRECT_LEVEL bits. I.e.:
//!
//! PPN = [indirect level n idx][...][indirect level 1 idx][in-section idx]
//!
//! the design can be regarded as a redix tree.
//!
//! TODO: vmemmap

use core::ptr::NonNull;

use crate::prelude::*;

const FRAME_IN_SECTION_IDX_BITS: usize = {
    let frame_section_shift_bytes = FRAME_SECTION_SHIFT_MB + 20;
    frame_section_shift_bytes - PagingArch::PAGE_SIZE_BITS
};
const BITS_PER_INDIRECT_LEVEL: usize = {
    static_assert!(size_of::<PhysPageNum>().is_multiple_of(8));
    PagingArch::PAGE_SIZE_BITS - size_of::<PhysPageNum>().trailing_zeros() as usize
};

const NINDIRECT_LEVELS: usize = {
    let total_indirect_bits = PagingArch::MAX_PPN_BITS - FRAME_IN_SECTION_IDX_BITS;
    align_up!(total_indirect_bits, BITS_PER_INDIRECT_LEVEL) / BITS_PER_INDIRECT_LEVEL
};
static_assert!(NINDIRECT_LEVELS > 0);
const ENTRIES_PER_INDIRECT_LEVEL: usize = PagingArch::PAGE_SIZE_BYTES / size_of::<PhysPageNum>();

// how many pages a MemSection occupies.
const MEMSECTION_NPAGES: usize = size_of::<MemSection>() / PagingArch::PAGE_SIZE_BYTES;
// how many `Frame`s a MemSection contains.
const NFRAMES_PER_SECTION: usize = 1 << FRAME_IN_SECTION_IDX_BITS;

/// Indirect level is just an array with size of PAGE_SIZE_BYTES, containing
/// PPNs pointing to next level indirect blocks or frame sections.
#[derive(Debug)]
#[repr(C)]
struct IndirectLevel {
    entries: [PhysPageNum; ENTRIES_PER_INDIRECT_LEVEL],
}
static_assert!(size_of::<IndirectLevel>() == PagingArch::PAGE_SIZE_BYTES);
impl IndirectLevel {
    const EMPTY: Self = Self {
        entries: [PhysPageNum::new(0); ENTRIES_PER_INDIRECT_LEVEL],
    };
}

#[derive(Debug)]
struct MemSection {
    frames: [Frame; 1 << FRAME_IN_SECTION_IDX_BITS],
}
static_assert!(size_of::<MemSection>().is_multiple_of(PagingArch::PAGE_SIZE_BYTES));

mod memmap {

    use super::*;
    static MEMMAP: MonoOnce<IndirectLevel> = unsafe { MonoOnce::new() };

    #[inline]
    fn indirect_indices_for(ppn: PhysPageNum) -> [usize; NINDIRECT_LEVELS] {
        // Top-level index may have fewer bits than regular levels.
        let top_level_idx = ppn.get()
            >> (FRAME_IN_SECTION_IDX_BITS + (NINDIRECT_LEVELS - 1) * BITS_PER_INDIRECT_LEVEL);

        let mut indices = [0; NINDIRECT_LEVELS];
        let mut shift = FRAME_IN_SECTION_IDX_BITS;
        for i in 0..NINDIRECT_LEVELS - 1 {
            indices[i] = ((ppn.get() >> shift) & ((1 << BITS_PER_INDIRECT_LEVEL) - 1)) as usize;
            shift += BITS_PER_INDIRECT_LEVEL;
        }
        indices[NINDIRECT_LEVELS - 1] = top_level_idx as usize;
        indices
    }

    /// dry run.
    ///
    /// After this function is called, we allocate required pages for memmap,
    /// using them stroing [Frame]s.
    ///
    /// However, when we perform the dry run, we have not yet allocated there
    /// frames. This portion of memory will leak, and [Frame]s don't track
    /// them, so we may end up over-allocating memory for memmap.
    ///
    /// But is't fine. Those leaked pages will never be allocated, so we
    /// won't have chance to access their corresponding [Frame]s.
    fn calculate_memmap_npages() -> usize {
        let section_mask = (NFRAMES_PER_SECTION as u64) - 1;
        let section_stride = NFRAMES_PER_SECTION as u64;

        // Keyed by section base PPN (aligned to section size).
        let mut section_keys = HashSet::new();
        // Keyed by (level i in init loop, ppn_prefix_at_level_i).
        // This models each unique indirect page allocated by:
        // for i in (1..NINDIRECT_LEVELS).rev() { if entry == 0 { alloc(1) } }
        let mut indirect_keys = HashSet::new();

        sys_mem_zones().with_avail_zones(|zones| {
            for zone in zones {
                let range = zone.range();
                if range.npages() == 0 {
                    continue;
                }

                let end_ppn = range.end().get();
                let mut section_base = range.start().get() & !section_mask;

                while section_base < end_ppn {
                    section_keys.insert(section_base);

                    for i in 1..NINDIRECT_LEVELS {
                        let shift = FRAME_IN_SECTION_IDX_BITS + i * BITS_PER_INDIRECT_LEVEL;
                        let prefix = section_base >> shift;
                        indirect_keys.insert((i, prefix));
                    }

                    section_base += section_stride;
                }
            }
        });

        section_keys.len() * MEMSECTION_NPAGES + indirect_keys.len()
    }

    /// Initialize system memmap.
    ///
    /// This function must be called right before
    /// [device::discovery::open_firmware::EarlyMemoryScanner::commit_to_pmm]
    /// and before [pmm_init].
    ///
    /// The `allocator` receives the number of pages to allocate, and returns
    /// the PPN of the first allocated page. If no available memory can be
    /// allocated, an immediate panic should be triggered, since kernel
    /// cannot run without memmap.
    pub unsafe fn init<A>(allocator: A)
    where
        A: FnOnce(usize) -> PhysPageNum,
    {
        let npages = calculate_memmap_npages();
        let sppn = allocator(npages);

        struct Bump {
            next_ppn: PhysPageNum,
            npages: usize,
        }

        impl Bump {
            fn alloc(&mut self, npages: usize) -> PhysPageNum {
                if self.npages < npages {
                    panic!(
                        "Internal error: we overcalculated memmap npages, but still run out of memory when initializing memmap"
                    );
                }
                let allocated_ppn = self.next_ppn;
                self.next_ppn += npages as u64;
                self.npages -= npages;
                allocated_ppn
            }
        }

        impl Drop for Bump {
            fn drop(&mut self) {
                kdebugln!(
                    "memmap init: finished allocating, {} pages wasted",
                    self.npages,
                );
            }
        }

        let mut bump = Bump {
            next_ppn: sppn,
            npages: npages,
        };

        MEMMAP.init(|root| {
            unsafe {
                let entry_ptr = root.as_mut_ptr().cast::<PhysPageNum>();
                for i in 0..ENTRIES_PER_INDIRECT_LEVEL {
                    entry_ptr.add(i).write(PhysPageNum::new(0));
                }
            }
            let root = unsafe { root.assume_init_mut() } as *mut IndirectLevel;

            sys_mem_zones().with_avail_zones(|zones| {
                let section_mask = (NFRAMES_PER_SECTION as u64) - 1;
                let section_stride = NFRAMES_PER_SECTION as u64;

                for zone in zones {
                    kdebugln!("initializing memmap for zone: {:?}", zone.range());
                    let range = zone.range();
                    if range.npages() == 0 {
                        continue;
                    }

                    let end_ppn = range.end().get();
                    let mut section_base = range.start().get() & !section_mask;

                    while section_base < end_ppn {
                        kdebugln!(
                            "initializing memmap in section starting at ppn {:#x}",
                            section_base
                        );
                        let section_base_ppn = PhysPageNum::new(section_base);
                        let indices = indirect_indices_for(section_base_ppn);
                        let mut cur_level = root;

                        for i in (1..NINDIRECT_LEVELS).rev() {
                            let idx = indices[i];

                            let next_ppn = unsafe {
                                let level = &mut *cur_level;
                                let entry = &mut level.entries[idx];
                                if entry.get() == 0 {
                                    // do an alloc
                                    *entry = bump.alloc(1);
                                    // initialize the allocated page as an indirect level
                                    (*entry)
                                        .to_hhdm()
                                        .to_virt_addr()
                                        .as_ptr_mut::<IndirectLevel>()
                                        .write(IndirectLevel::EMPTY);
                                }
                                *entry
                            };

                            cur_level = next_ppn
                                .to_hhdm()
                                .to_virt_addr()
                                .as_ptr_mut::<IndirectLevel>();
                        }

                        let section_idx = indices[0];
                        unsafe {
                            let level = &mut *cur_level;
                            let entry = &mut level.entries[section_idx];
                            if entry.get() == 0 {
                                let section_ppn = bump.alloc(MEMSECTION_NPAGES);

                                // we can't construct a memsection_ptr and write to it directly,
                                // since
                                // size_of::<MemSection>() is too large and will overflow the
                                // stack. That's why we write to each frame one by one.
                                //
                                // tbh, i kind of miss cpp's placement new here...

                                let frame_ptr =
                                    section_ppn.to_hhdm().to_virt_addr().as_ptr_mut::<Frame>();
                                for i in 0..NFRAMES_PER_SECTION {
                                    let frame_ptr = frame_ptr.add(i);
                                    frame_ptr.write(Frame::EMPTY);
                                    (&mut *frame_ptr).ppn = section_base_ppn + i as u64;
                                }

                                *entry = section_ppn;
                            }
                        }

                        section_base += section_stride;
                    }
                }
            });
        });
    }

    /// Gets the underlying [Frame] corresponding to the given physical page
    /// number.
    ///
    /// This function is intentionally designed not to return an
    /// [Option]/[Result], since if a [None]/[Err] is returned, it indicates
    /// a serious bug in the kernel, and we should just panic immediately.
    ///
    /// However, this function is marked as `unsafe`.
    pub unsafe fn get_frame(ppn: PhysPageNum) -> &'static Frame {
        let in_section_idx = ppn.get() & ((1 << FRAME_IN_SECTION_IDX_BITS) - 1);

        let indirect_level_idx_indice = indirect_indices_for(ppn);

        let section = {
            let mut cur_ppn = MEMMAP.get().entries[indirect_level_idx_indice[NINDIRECT_LEVELS - 1]];
            for i in (0..NINDIRECT_LEVELS - 1).rev() {
                if cur_ppn.get() == 0 {
                    panic!("Internal error: try to access an invalid frame");
                }
                cur_ppn = unsafe {
                    NonNull::new_unchecked(
                        cur_ppn
                            .to_hhdm()
                            .to_virt_addr()
                            .as_ptr_mut::<IndirectLevel>(),
                    )
                    .as_ref()
                }
                .entries[indirect_level_idx_indice[i]];
            }
            if cur_ppn.get() == 0 {
                panic!("Internal error: try to access an invalid frame");
            }
            unsafe {
                NonNull::new_unchecked(cur_ppn.to_hhdm().to_virt_addr().as_ptr_mut::<MemSection>())
                    .as_ref()
            }
        };

        let frame_ptr = &section.frames[in_section_idx as usize] as *const Frame;

        // SAFETY: after memmap is initialized, frame structures will remain valid for
        // the whole system lifetime.

        unsafe { &*frame_ptr }
    }
}
pub use memmap::{get_frame, init};

#[derive(Debug)]
pub struct Frame {
    /// The physical page number of this frame.
    ///
    /// What's this used for? tbh idk, but let's just store it here. maybe we'll
    /// need it for debugging or something.
    ppn: PhysPageNum,
    rc: AtomicUsize,
    // TODO: add flags for frame state
}

impl Frame {
    const EMPTY: Self = Self {
        ppn: PhysPageNum::new(0),
        rc: AtomicUsize::new(0),
    };

    /// Internally, this indicates that the reference count of this frame is 0.
    pub fn is_free(&self) -> bool {
        self.rc.load(Ordering::Acquire) == 0
    }

    /// Internally, this indicates that the reference count of this frame is
    /// greater than 0.
    pub fn is_used(&self) -> bool {
        !self.is_free()
    }

    /// Internally, this indicates that the reference count of this frame is
    /// greater than 1.
    pub fn is_shared(&self) -> bool {
        self.rc.load(Ordering::Acquire) > 1
    }

    /// Increase the reference count of this frame by 1.
    pub unsafe fn inc_ref(&self) {
        self.rc.fetch_add(1, Ordering::AcqRel);
    }

    /// Decrease the reference count of this frame by 1.
    pub unsafe fn dec_ref(&self) {
        if self.is_free() {
            panic!("Internal error: trying to decrease reference count of a free frame");
        }

        self.rc.fetch_sub(1, Ordering::AcqRel);
    }

    /// Returns the current reference count of this frame.
    ///
    /// This is mainly used for debugging and testing, and should not be used in
    /// normal code.
    pub fn rc(&self) -> usize {
        self.rc.load(Ordering::Acquire)
    }
}
