use core::ffi::CStr;

use alloc::boxed::Box;
use anemone_abi::{
    errno::Errno,
    syscall::{SYS_EXECVE, syscall},
};

pub fn execve(path: &CStr, argv: &[&CStr]) -> Result<u64, Errno> {
    let mut args = unsafe { Box::new_uninit_slice(argv.len() + 1).assume_init() };
    for i in 0..argv.len() {
        args[i] = argv[i].as_ptr() as u64;
    }
    args[argv.len()] = 0;
    unsafe {
        syscall(
            SYS_EXECVE,
            path.as_ptr() as u64,
            &args[0] as *const u64 as u64,
            0,
            0,
            0,
            0,
        )
    }
}
