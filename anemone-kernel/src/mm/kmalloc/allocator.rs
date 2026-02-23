use core::{alloc::GlobalAlloc, ptr::NonNull, sync::atomic::AtomicBool};

use talc::{OomHandler, Span, Talc};

use crate::{
    prelude::*,
    utils::align::{AlignedBytes, PhantomAligned4096},
};

#[unsafe(link_section = ".bss.bootstrap_heap")]
static mut BOOTSTRAP_HEAP: AlignedBytes<
    PhantomAligned4096,
    [u8; (1 << BOOTSTRAP_HEAP_SHIFT_KB) as usize * 1024],
> = AlignedBytes::ZEROED;

#[derive(Debug)]
pub struct KernelAllocator {
    // TODO: switch to IrqSaveSpinLock to prevent deadlocks in OOM handler.
    talc: SpinLock<Talc<HeapOomHandler>>,
}

struct HeapOomHandler {
    bootstrap_heap_claimed: AtomicBool,
}

impl OomHandler for HeapOomHandler {
    fn handle_oom(talc: &mut Talc<Self>, layout: core::alloc::Layout) -> Result<(), ()> {
        unsafe {
            if !talc
                .oom_handler
                .bootstrap_heap_claimed
                .swap(true, Ordering::SeqCst)
            {
                let used = talc
                    .claim(Span::from_array(&raw mut BOOTSTRAP_HEAP.bytes))
                    .expect("bootstrap heap should be claimable");
                kinfoln!("HeapOomHandler: claimed bootstrap heap {}", used);
                return Ok(());
            } else {
                kdebugln!(
                    "HeapOomHandler: bootstrap heap already claimed, trying to request memory from frame allocator"
                );
                // if pmm is not yet initialized, this will fail and return Err(()).

                // the folio we requested here won't be deallocated until the end of the
                // kernel's execution, so we don't need to worry about freeing it.

                // 1. the minimum number of pages we need to allocate to satisfy the request.
                let min_npages =
                    (layout.size() + PagingArch::PAGE_SIZE_BYTES) / PagingArch::PAGE_SIZE_BYTES;

                // 2. we tend to allocate a power of two number of pages to reduce
                //    fragmentation, so we round up to the next power of two.
                let npages = min_npages.next_power_of_two();

                // NOTE
                // In a serious OOM (Out of Memory) handler, we should avoid calling expect or
                // panic here because the frame allocator itself might be completely exhausted,
                // and attempting to handle a panic could trigger further allocations, leading
                // to a recursive kernel fault.
                //
                // A robust kernel implementation would instead treat memory exhaustion as a
                // recoverable state. For user-space processes, memory
                // interaction is mediated through page allocation, which is
                // inherently a fallible operation. The kernel can maintain system stability by
                // monitoring page allocator watermarks. When free memory drops below a critical
                // threshold, the kernel can proactively invoke an OOM killer to terminate
                // non-essential user processes, thereby reclaiming physical frames to satisfy
                // internal demands.
                //
                // Internal kernel OOM panics typically occur
                // when the kernel's own data structures grow beyond available capacity.
                // Ideally, instead of panicking, the allocator should allow the current thread
                // to sleep until memory is reclaimed, or terminate the offending thread if the
                // allocation is deemed non-critical. The reason we currently
                // resort to a simple panic is largely due to the current state of the Rust
                // ecosystem.
                //
                // The default alloc crate is designed around infallible allocations,
                // meaning it assumes memory is always available and panics by default when it
                // is not. While support for fallible allocation (such as the try_alloc family
                // of methods) is evolving, it remains relatively immature and difficult to
                // integrate throughout a complex kernel codebase. Consequently, this panic
                // serves as a temporary compromise until more sophisticated fallible allocation
                // patterns are fully supported.
                let folio = alloc_frames(npages).expect(
                    "frame allocator has no free memory to satisfy HeapOomHandler's request",
                );
                let range = folio.leak();

                let len = range.npages() as usize * PagingArch::PAGE_SIZE_BYTES;
                let ptr = range.start().to_hhdm().to_vaddr();
                let slice: *mut [u8] = core::ptr::slice_from_raw_parts_mut(ptr.as_ptr_mut(), len);
                let used = talc
                    .claim(Span::from_slice(slice))
                    .expect("should be able to claim folio from frame allocator");
                kinfoln!(
                    "HeapOomHandler: claimed folio of {} pages ({} bytes) from frame allocator",
                    range.npages(),
                    used
                );
                return Ok(());
            }
        }
    }
}

impl KernelAllocator {
    pub const fn new() -> Self {
        Self {
            talc: SpinLock::new(Talc::new(HeapOomHandler {
                bootstrap_heap_claimed: AtomicBool::new(false),
            })),
        }
    }
}

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        let mut talc = self.talc.lock_irqsave();
        match unsafe { talc.malloc(layout) } {
            Ok(ptr) => ptr.as_ptr(),
            // No need to handle OOM here since the OOM handler will be invoked by `malloc` when
            // allocation fails. We can simply return null pointer to indicate allocation failure.
            Err(()) => core::ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        let mut talc = self.talc.lock_irqsave();
        unsafe {
            talc.free(NonNull::new_unchecked(ptr), layout);
        }
    }
}
