use core::{alloc::GlobalAlloc, cmp::max, ptr::NonNull};

use anemone_abi::{
    errno::Errno,
    syscall::{SYS_BRK, syscall},
};
use spin::Mutex;
use talc::{OomHandler, Span, Talc};

pub fn brk(addr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_BRK, addr, 0, 0, 0, 0, 0) }
}

#[global_allocator]
static ALLOC: UserGlobalAlloc = UserGlobalAlloc;

pub struct UserGlobalAlloc;
unsafe impl GlobalAlloc for UserGlobalAlloc {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        unsafe {
            TALC.lock()
                .malloc(layout)
                .unwrap_or_else(|_| panic!("failed to allocate memory '{:?}'", layout))
                .as_ptr()
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        unsafe {
            TALC.lock().free(
                NonNull::new(ptr)
                    .expect("failed to deallocate memory: trying to free a null pointer"),
                layout,
            );
        }
    }
}

static TALC: Mutex<Talc<UserOomHandler>> = Mutex::new(Talc::new(UserOomHandler));

pub struct UserOomHandler;

const HEAP_GROW_MIN_STEP: u64 = 4 * MB_ALIGN; // 4MiB
const MB_ALIGN: u64 = 1024 * 1024;

// lock not needed
static mut BRK: u64 = 0;

pub(crate) fn init() {
    unsafe {
        BRK = brk(0).expect("user heap init failed");
    }
}

impl OomHandler for UserOomHandler {
    fn handle_oom(talc: &mut Talc<Self>, layout: core::alloc::Layout) -> Result<(), ()> {
        let size = max(layout.size() as u64, HEAP_GROW_MIN_STEP);
        unsafe {
            let old_brk = BRK;
            let new_brk = BRK + size;
            BRK = brk(new_brk).unwrap_or_else(|e| panic!("failed to allocate memory: {:#x}", e));
            talc.claim(Span::new(old_brk as *mut u8, new_brk as *mut u8))?;
        }
        Ok(())
    }
}
