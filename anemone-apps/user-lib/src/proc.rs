use core::str::FromStr;

use alloc::{ffi::CString, vec, vec::Vec};
use anemone_abi::{
    errno::{self, Errno},
    syscall::{SYS_EXECVE, SYS_EXIT, SYS_SCHED_YIELD, syscall},
};

pub fn execve(path: impl AsRef<str>, argv: &[impl AsRef<str>]) -> Result<u64, Errno> {
    let mut args = vec![0; argv.len() + 1].into_boxed_slice();
    let argv = argv
        .iter()
        .map(|s| CString::from_str(s.as_ref()).map_err(|_| errno::EINVAL))
        .collect::<Result<Vec<CString>, Errno>>()?;
    for i in 0..argv.len() {
        args[i] = argv[i].as_c_str().as_ptr() as u64;
    }
    args[argv.len()] = 0;
    let path = CString::from_str(path.as_ref()).map_err(|_| errno::EINVAL)?;
    let path_ptr = path.as_c_str().as_ptr() as u64;
    unsafe {
        syscall(
            SYS_EXECVE,
            path_ptr,
            &args[0] as *const u64 as u64,
            0,
            0,
            0,
            0,
        )
    }
}

pub fn exit(code: i32) -> ! {
    unsafe {
        syscall(SYS_EXIT as u64, code as u64, 0, 0, 0, 0, 0)
            .expect("failed to send `exit` syscall");
    }
    unreachable!("sys_exit should never return");
}

pub fn sched_yield() -> Result<(), Errno> {
    unsafe { syscall(SYS_SCHED_YIELD as u64, 0, 0, 0, 0, 0, 0).map(|_| ()) }
}
