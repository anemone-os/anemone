use core::{
    ffi::{CStr, c_char},
    panic::PanicInfo,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::{Errno, allocator, process::exit};

static START_ARGS_PTR: AtomicUsize = AtomicUsize::new(0);

unsafe extern "Rust" {
    fn anemone_user_main() -> Result<(), Errno>;
}

pub struct Args {
    current: usize,
}

impl Iterator for Args {
    type Item = &'static str;

    fn next(&mut self) -> Option<Self::Item> {
        let start_args_ptr = START_ARGS_PTR.load(Ordering::Acquire) as *const u64;
        if start_args_ptr.is_null() {
            return None;
        }

        unsafe {
            let len = *start_args_ptr as usize;
            if self.current >= len {
                return None;
            }

            let ptr = *start_args_ptr.add(1 + self.current) as *const u8;
            let c_str = CStr::from_ptr(ptr as *const c_char);
            self.current += 1;
            Some(c_str.to_str().expect("failed to decode process arguments"))
        }
    }
}

pub fn args() -> Args {
    Args { current: 0 }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(stack_top: *const u64) -> ! {
    START_ARGS_PTR.store(stack_top as usize, Ordering::Release);
    allocator::init();

    match unsafe { anemone_user_main() } {
        Ok(()) => exit(0),
        Err(errno) => exit(errno as i8),
    }
}

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    crate::println!("panic: {info:?}");
    exit(-1)
}
