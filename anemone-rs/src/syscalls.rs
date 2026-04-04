//! Raw syscall interfaces.
//!
//! It's always preferred to use upper-level encapsulations of these syscalls.

use anemone_abi::{errno::Errno, syscall::*};

pub unsafe fn raw_syscall(
    number: u64,
    arg0: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
) -> Result<u64, Errno> {
    unsafe { syscall(number, arg0, arg1, arg2, arg3, arg4, arg5) }
}

pub fn sys_brk(addr: u64) -> Result<u64, Errno> {
    unsafe { raw_syscall(SYS_BRK, addr, 0, 0, 0, 0, 0) }
}

pub fn sys_dbg_print(ptr: u64, len: u64) -> Result<(), Errno> {
    unsafe { raw_syscall(SYS_DBG_PRINT, ptr, len, 0, 0, 0, 0).map(|_| ()) }
}

pub fn sys_execve(path_ptr: u64, argv_ptr: u64) -> Result<u64, Errno> {
    unsafe { raw_syscall(SYS_EXECVE, path_ptr, argv_ptr, 0, 0, 0, 0) }
}

pub fn sys_clone() -> Result<u64, Errno> {
    unsafe { raw_syscall(SYS_CLONE, 0, 0, 0, 0, 0, 0) }
}

pub fn sys_exit(code: u64) -> ! {
    unsafe {
        raw_syscall(SYS_EXIT, code, 0, 0, 0, 0, 0).expect("failed to invoke exit syscall");
    }
    unreachable!("sys_exit should never return")
}

pub fn sys_sched_yield() -> Result<(), Errno> {
    unsafe { raw_syscall(SYS_SCHED_YIELD, 0, 0, 0, 0, 0, 0).map(|_| ()) }
}
