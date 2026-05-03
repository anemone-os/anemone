use core::{alloc::GlobalAlloc, cmp::max, ptr::NonNull};

use spin::Mutex;
use talc::{OomHandler, Span, Talc};

use crate::sys::linux::process::brk;

#[global_allocator]
static ALLOCATOR: GlobalAllocator = GlobalAllocator;

static TALC: Mutex<Talc<GlobalOomHandler>> = Mutex::new(Talc::new(GlobalOomHandler));

const MIB: u64 = 1024 * 1024;
const HEAP_GROW_MIN_STEP: u64 = 4 * MIB;

static mut PROGRAM_BREAK: u64 = 0;

pub(crate) fn init() {
    unsafe {
        PROGRAM_BREAK = brk(0).expect("user heap init failed");
    }
}

struct GlobalAllocator;

unsafe impl GlobalAlloc for GlobalAllocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        unsafe {
            TALC.lock()
                .malloc(layout)
                .unwrap_or_else(|_| panic!("failed to allocate memory {layout:?}"))
                .as_ptr()
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        unsafe {
            TALC.lock().free(
                NonNull::new(ptr).expect("attempted to free a null pointer"),
                layout,
            );
        }
    }
}

struct GlobalOomHandler;

impl OomHandler for GlobalOomHandler {
    fn handle_oom(talc: &mut Talc<Self>, layout: core::alloc::Layout) -> Result<(), ()> {
        let grow_by = max(layout.size() as u64, HEAP_GROW_MIN_STEP);
        unsafe {
            let old_break = PROGRAM_BREAK;
            let new_break = PROGRAM_BREAK + grow_by;
            PROGRAM_BREAK =
                brk(new_break).unwrap_or_else(|err| panic!("failed to grow heap: {err:#x}"));
            talc.claim(Span::new(old_break as *mut u8, new_break as *mut u8))?;
        }
        Ok(())
    }
}
