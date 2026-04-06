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

pub fn sys_clone(parent_tid_ptr: u64, child_tid_ptr: u64) -> Result<u64, Errno> {
    unsafe { raw_syscall(SYS_CLONE, parent_tid_ptr, child_tid_ptr, 0, 0, 0, 0) }
}

pub fn sys_openat(dirfd: isize, path_ptr: u64, flags: u32, mode: u32) -> Result<usize, Errno> {
    unsafe {
        raw_syscall(
            SYS_OPENAT,
            dirfd as u64,
            path_ptr,
            flags as u64,
            mode as u64,
            0,
            0,
        )
        .map(|fd| fd as usize)
    }
}

pub fn sys_read(fd: usize, buf_ptr: u64, count: usize) -> Result<usize, Errno> {
    unsafe { raw_syscall(SYS_READ, fd as u64, buf_ptr, count as u64, 0, 0, 0).map(|n| n as usize) }
}

pub fn sys_write(fd: usize, buf_ptr: u64, count: usize) -> Result<usize, Errno> {
    unsafe { raw_syscall(SYS_WRITE, fd as u64, buf_ptr, count as u64, 0, 0, 0).map(|n| n as usize) }
}

pub fn sys_close(fd: usize) -> Result<(), Errno> {
    unsafe { raw_syscall(SYS_CLOSE, fd as u64, 0, 0, 0, 0, 0).map(|_| ()) }
}

pub fn sys_dup(fd: usize) -> Result<usize, Errno> {
    unsafe { raw_syscall(SYS_DUP, fd as u64, 0, 0, 0, 0, 0).map(|fd| fd as usize) }
}

pub fn sys_dup3(oldfd: usize, newfd: usize, flags: u32) -> Result<usize, Errno> {
    unsafe {
        raw_syscall(SYS_DUP3, oldfd as u64, newfd as u64, flags as u64, 0, 0, 0)
            .map(|fd| fd as usize)
    }
}

pub fn sys_exit(code: u64) -> ! {
    unsafe {
        raw_syscall(SYS_EXIT, code, 0, 0, 0, 0, 0).expect("failed to invoke exit syscall");
    }
    unreachable!("sys_exit should never return")
}

pub fn sys_getpid() -> Result<u32, Errno> {
    unsafe { raw_syscall(SYS_GETPID, 0, 0, 0, 0, 0, 0).map(|pid| pid as u32) }
}

pub fn sys_getppid() -> Result<u32, Errno> {
    unsafe { raw_syscall(SYS_GETPPID, 0, 0, 0, 0, 0, 0).map(|ppid| ppid as u32) }
}

pub fn sys_sched_yield() -> Result<(), Errno> {
    unsafe { raw_syscall(SYS_SCHED_YIELD, 0, 0, 0, 0, 0, 0).map(|_| ()) }
}

pub fn sys_getcwd(buf_ptr: u64, size: u64) -> Result<(), Errno> {
    unsafe { raw_syscall(SYS_GETCWD, buf_ptr, size, 0, 0, 0, 0).map(|_| ()) }
}

pub fn sys_chdir(path_ptr: u64) -> Result<(), Errno> {
    unsafe { raw_syscall(SYS_CHDIR, path_ptr, 0, 0, 0, 0, 0).map(|_| ()) }
}

pub fn sys_chroot(path_ptr: u64) -> Result<(), Errno> {
    unsafe { raw_syscall(SYS_CHROOT, path_ptr, 0, 0, 0, 0, 0).map(|_| ()) }
}
