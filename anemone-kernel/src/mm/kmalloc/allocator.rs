use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::NonNull,
    sync::atomic::AtomicBool,
};

use talc::{OomHandler, Span, Talc};

use crate::{
    mm::kmalloc::slab::{
        BOOTSTRAP_HEAP_BYTES, PreparedSpan, SlabAllocator, SlabClass, slab_span_layout,
    },
    prelude::*,
    utils::align::{AlignedBytes, PhantomAligned4096},
};

#[unsafe(link_section = ".bss.bootstrap_heap")]
static mut BOOTSTRAP_HEAP: AlignedBytes<PhantomAligned4096, [u8; BOOTSTRAP_HEAP_BYTES]> =
    AlignedBytes::ZEROED;

#[derive(Debug)]
pub struct KernelAllocator {
    slab: SlabAllocator,
    talc: NoIrqSpinLock<Talc<HeapOomHandler>>,
}

struct HeapOomHandler {
    bootstrap_heap_claimed: AtomicBool,
}

fn expansion_npages(layout: Layout) -> Option<usize> {
    let page_size = PagingArch::PAGE_SIZE_BYTES;

    // A newly claimed Talc arena must hold more than the requested payload:
    // its base can require alignment padding, and Talc keeps arena/chunk
    // bookkeeping outside that payload. Reserve one page for allocator
    // bookkeeping instead of duplicating Talc's private tag layout here.
    let arena_bytes = layout
        .size()
        .checked_add(layout.align() - 1)?
        .checked_add(page_size)?;
    let pages = arena_bytes.checked_add(page_size - 1)? / page_size;
    pages.checked_next_power_of_two()
}

impl OomHandler for HeapOomHandler {
    fn handle_oom(talc: &mut Talc<Self>, layout: Layout) -> Result<(), ()> {
        unsafe {
            if !talc
                .oom_handler
                .bootstrap_heap_claimed
                .swap(true, Ordering::SeqCst)
            {
                let used = match talc.claim(Span::from_array(&raw mut BOOTSTRAP_HEAP.bytes)) {
                    Ok(used) => used,
                    Err(()) => {
                        // Claim is the publication point for this one-shot
                        // arena. A failed attempt must not make a later retry
                        // skip the only bootstrap memory.
                        talc.oom_handler
                            .bootstrap_heap_claimed
                            .store(false, Ordering::SeqCst);
                        return Err(());
                    },
                };
                kinfoln!(noprint, "HeapOomHandler: claimed bootstrap heap {}", used);
                return Ok(());
            } else {
                knoticeln!(
                    noprint,
                    "HeapOomHandler: bootstrap heap already claimed, trying to request memory from frame allocator"
                );
                // if pmm is not yet initialized, this will fail and return Err(()).

                let npages = expansion_npages(layout).ok_or(())?;
                let folio = alloc_frames(npages).ok_or(())?;
                let range = folio.range();

                let len = range.npages() as usize * PagingArch::PAGE_SIZE_BYTES;
                let ptr = range.start().to_hhdm().to_virt_addr();
                let slice: *mut [u8] = core::ptr::slice_from_raw_parts_mut(ptr.as_ptr_mut(), len);
                let _used = talc.claim(Span::from_slice(slice)).map_err(|_| ())?;

                // Talc now owns the claimed bytes. Transfer the folio only
                // after that commit so every earlier failure returns it to the
                // frame allocator instead of leaking an unusable arena.
                let range = folio.leak();
                knoticeln!(
                    noprint,
                    "HeapOomHandler: claimed folio {:?} from frame allocator",
                    range
                );
                return Ok(());
            }
        }
    }
}

impl KernelAllocator {
    pub const fn new() -> Self {
        Self {
            slab: SlabAllocator::new(),
            talc: NoIrqSpinLock::new(Talc::new(HeapOomHandler {
                bootstrap_heap_claimed: AtomicBool::new(false),
            })),
        }
    }

    fn talc_alloc(&self, layout: Layout) -> Option<NonNull<u8>> {
        // Keep the failure handoff outside the Talc guard: Rust's infallible
        // allocation failure diagnostics may themselves allocate.
        let result = {
            let mut talc = self.talc.lock();
            unsafe { talc.malloc(layout) }
        };
        result.ok()
    }

    unsafe fn talc_dealloc(&self, ptr: NonNull<u8>, layout: Layout) {
        let mut talc = self.talc.lock();
        unsafe { talc.free(ptr, layout) };
    }

    fn grow_slab(&self, class: SlabClass) -> Option<NonNull<u8>> {
        let span_layout = slab_span_layout();
        let span = self.talc_alloc(span_layout)?;
        let Some(prepared) = PreparedSpan::new(span, class) else {
            // Preparation is private and has not published a slot. Returning
            // this allocation to Talc is therefore the only cleanup owner.
            unsafe { self.talc_dealloc(span, span_layout) };
            return None;
        };
        Some(self.slab.publish(prepared))
    }
}

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if let Some(class) = SlabClass::for_layout(layout) {
            return self
                .slab
                .allocate(class)
                .or_else(|| self.grow_slab(class))
                .map_or(core::ptr::null_mut(), NonNull::as_ptr);
        }

        self.talc_alloc(layout)
            .map_or(core::ptr::null_mut(), NonNull::as_ptr)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let ptr = unsafe { NonNull::new_unchecked(ptr) };
        if let Some(class) = SlabClass::for_layout(layout) {
            unsafe { self.slab.deallocate(class, ptr) };
        } else {
            unsafe { self.talc_dealloc(ptr, layout) };
        }
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use talc::ErrOnOom;

    const REGRESSION_SIZE: usize = u16::MAX as usize;
    const OLD_ARENA_PAGES: usize = 16;
    const PROGRESS_ARENA_PAGES: usize = 32;

    static mut METADATA_ARENA: AlignedBytes<PhantomAligned4096, [u8; PagingArch::PAGE_SIZE_BYTES]> =
        AlignedBytes::ZEROED;
    static mut EXPANSION_ARENA: AlignedBytes<
        PhantomAligned4096,
        [u8; PROGRESS_ARENA_PAGES * PagingArch::PAGE_SIZE_BYTES],
    > = AlignedBytes::ZEROED;

    #[kunit]
    fn expansion_arena_makes_the_triggering_layout_allocatable() {
        let layout = Layout::from_size_align(REGRESSION_SIZE, 1).unwrap();
        assert_eq!(expansion_npages(layout), Some(PROGRESS_ARENA_PAGES));

        unsafe {
            {
                let mut old = Talc::new(ErrOnOom);
                old.claim(Span::from_array(&raw mut METADATA_ARENA.bytes))
                    .unwrap();
                let old_arena = core::ptr::slice_from_raw_parts_mut(
                    (&raw mut EXPANSION_ARENA.bytes).cast::<u8>(),
                    OLD_ARENA_PAGES * PagingArch::PAGE_SIZE_BYTES,
                );
                old.claim(Span::from_slice(old_arena)).unwrap();
                assert!(old.malloc(layout).is_err());
            }

            let mut progress = Talc::new(ErrOnOom);
            progress
                .claim(Span::from_array(&raw mut METADATA_ARENA.bytes))
                .unwrap();
            progress
                .claim(Span::from_array(&raw mut EXPANSION_ARENA.bytes))
                .unwrap();
            assert!(progress.malloc(layout).is_ok());
        }
    }

    #[kunit]
    fn expansion_budget_covers_page_edges_and_alignment() {
        let page_size = PagingArch::PAGE_SIZE_BYTES;
        assert_eq!(
            expansion_npages(Layout::from_size_align(page_size - 1, 1).unwrap()),
            Some(2)
        );
        assert_eq!(
            expansion_npages(Layout::from_size_align(page_size, 1).unwrap()),
            Some(2)
        );
        assert_eq!(
            expansion_npages(Layout::from_size_align(page_size, page_size).unwrap()),
            Some(4)
        );

        let largest = Layout::from_size_align(isize::MAX as usize, 1).unwrap();
        assert!(expansion_npages(largest).is_some());
    }
}
