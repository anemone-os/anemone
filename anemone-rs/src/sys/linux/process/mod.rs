use super::*;

pub fn capget(header_ptr: u64, data_ptr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_CAPGET, header_ptr, data_ptr, 0, 0, 0, 0) }
}

pub fn capset(header_ptr: u64, data_ptr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_CAPSET, header_ptr, data_ptr, 0, 0, 0, 0) }
}

pub fn brk(addr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_BRK, addr, 0, 0, 0, 0, 0) }
}

pub fn mmap(
    addr: u64,
    length: u64,
    prot: u64,
    flags: u64,
    fd: u64,
    offset: u64,
) -> Result<u64, Errno> {
    unsafe { syscall(SYS_MMAP, addr, length, prot, flags, fd, offset) }
}

pub fn munmap(addr: u64, length: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_MUNMAP, addr, length, 0, 0, 0, 0) }
}

pub fn mremap(
    old_addr: u64,
    old_size: u64,
    new_size: u64,
    flags: u64,
    new_addr: u64,
) -> Result<u64, Errno> {
    unsafe { syscall(SYS_MREMAP, old_addr, old_size, new_size, flags, new_addr, 0) }
}

pub fn mprotect(addr: u64, length: u64, prot: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_MPROTECT, addr, length, prot, 0, 0, 0) }
}

pub fn msync(addr: u64, length: u64, flags: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_MSYNC, addr, length, flags, 0, 0, 0) }
}

pub fn mlock(addr: u64, length: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_MLOCK, addr, length, 0, 0, 0, 0) }
}

pub fn munlock(addr: u64, length: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_MUNLOCK, addr, length, 0, 0, 0, 0) }
}

pub fn execve(path_ptr: u64, argv_ptr: u64, envp_ptr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_EXECVE, path_ptr, argv_ptr, envp_ptr, 0, 0, 0) }
}

pub fn clone(
    flags: u64,
    stack_ptr: u64,
    parent_tid_ptr: u64,
    tls_ptr: u64,
    child_tid_ptr: u64,
) -> Result<u64, Errno> {
    #[cfg(target_arch = "loongarch64")]
    let (arg3, arg4) = (child_tid_ptr, tls_ptr);
    #[cfg(not(target_arch = "loongarch64"))]
    let (arg3, arg4) = (tls_ptr, child_tid_ptr);

    unsafe { syscall(SYS_CLONE, flags, stack_ptr, parent_tid_ptr, arg3, arg4, 0) }
}

pub fn exit(code: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_EXIT, code, 0, 0, 0, 0, 0) }
}

pub fn exit_group(code: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_EXIT_GROUP, code, 0, 0, 0, 0, 0) }
}

pub fn sched_yield() -> Result<u64, Errno> {
    unsafe { syscall(SYS_SCHED_YIELD, 0, 0, 0, 0, 0, 0) }
}

pub fn getpriority(which: i32, who: i32) -> Result<u64, Errno> {
    unsafe {
        syscall(
            SYS_GETPRIORITY,
            which as i64 as u64,
            who as i64 as u64,
            0,
            0,
            0,
            0,
        )
    }
}

pub fn setpriority(which: i32, who: i32, nice: i32) -> Result<u64, Errno> {
    unsafe {
        syscall(
            SYS_SETPRIORITY,
            which as i64 as u64,
            who as i64 as u64,
            nice as i64 as u64,
            0,
            0,
            0,
        )
    }
}

pub fn gettid() -> Result<u64, Errno> {
    unsafe { syscall(SYS_GETTID, 0, 0, 0, 0, 0, 0) }
}

pub fn getpid() -> Result<u64, Errno> {
    unsafe { syscall(SYS_GETPID, 0, 0, 0, 0, 0, 0) }
}

pub fn getppid() -> Result<u64, Errno> {
    unsafe { syscall(SYS_GETPPID, 0, 0, 0, 0, 0, 0) }
}

pub fn setpgid(pid: i32, pgid: i32) -> Result<u64, Errno> {
    unsafe {
        syscall(
            SYS_SETPGID,
            pid as i64 as u64,
            pgid as i64 as u64,
            0,
            0,
            0,
            0,
        )
    }
}

pub fn setsid() -> Result<u64, Errno> {
    unsafe { syscall(SYS_SETSID, 0, 0, 0, 0, 0, 0) }
}

pub fn wait4(pid: u64, wstatus_ptr: u64, options: u64, rusage_ptr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_WAIT4, pid, wstatus_ptr, options, rusage_ptr, 0, 0) }
}

pub fn getrusage(who: i32, usage_ptr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_GETRUSAGE, who as i64 as u64, usage_ptr, 0, 0, 0, 0) }
}

#[cfg(target_arch = "riscv64")]
pub fn getrlimit(resource: u32, limit_ptr: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_GETRLIMIT, resource as u64, limit_ptr, 0, 0, 0, 0) }
}

pub fn prlimit64(
    pid: i32,
    resource: u32,
    new_limit_ptr: u64,
    old_limit_ptr: u64,
) -> Result<u64, Errno> {
    unsafe {
        syscall(
            SYS_PRLIMIT64,
            pid as i64 as u64,
            resource as u64,
            new_limit_ptr,
            old_limit_ptr,
            0,
            0,
        )
    }
}

pub mod signal;
