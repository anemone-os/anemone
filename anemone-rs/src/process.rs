use alloc::{ffi::CString, vec, vec::Vec};
use anemone_abi::errno::{self, Errno};

use crate::syscalls::{sys_execve, sys_exit, sys_sched_yield};

pub fn execve(path: impl AsRef<str>, argv: &[impl AsRef<str>]) -> Result<u64, Errno> {
    let mut argv_ptrs = vec![0; argv.len() + 1].into_boxed_slice();
    let argv = argv
        .iter()
        .map(|arg| CString::new(arg.as_ref()).map_err(|_| errno::EINVAL))
        .collect::<Result<Vec<CString>, Errno>>()?;

    for (index, arg) in argv.iter().enumerate() {
        argv_ptrs[index] = arg.as_ptr() as u64;
    }
    argv_ptrs[argv.len()] = 0;

    let path = CString::new(path.as_ref()).map_err(|_| errno::EINVAL)?;
    sys_execve(path.as_ptr() as u64, argv_ptrs.as_ptr() as u64)
}

pub fn exit(code: i32) -> ! {
    sys_exit(code as u64)
}

pub fn sched_yield() -> Result<(), Errno> {
    sys_sched_yield()
}
