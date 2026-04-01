#![no_std]

use core::{ffi::{CStr, c_char}, panic::PanicInfo};

use crate::proc::exit;

pub extern crate alloc;

pub mod con;
#[macro_use]
pub mod ualloc;
pub mod proc;

unsafe extern "Rust" {
    fn main() -> i32;
}

static mut START_ARGS_PTR: *const u64 = 0 as *const u64;

pub struct ArgsIterator {
    current: usize,
}
impl Iterator for ArgsIterator {
    type Item = &'static str;

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            let len = *START_ARGS_PTR.add(0) as usize;
            if self.current >= len {
                return None;
            }
            let ptr = *START_ARGS_PTR.add(1 + self.current) as *const u8;
            let c_str = CStr::from_ptr(ptr as *const c_char);
            self.current += 1;
            Some(c_str.to_str().expect("failed to parse arguments"))
        }
    }
}

pub fn args() -> ArgsIterator {
    ArgsIterator { current: 0 }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start(stack_top: *const u64) {
    unsafe {
        START_ARGS_PTR = stack_top;
        ualloc::init();
        let exit_code = main();
        exit(exit_code)
    }
}

#[panic_handler]
pub fn panic_handler(info: &PanicInfo) -> ! {
    println!("user panic: {:?}", info);
    exit(-1);
}
