//! System call conventions and numbers.
//! Architecture-specific.

unsafe fn syscall_raw(
    sysno: u64,
    arg0: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
) -> u64 {
    let ret: u64;
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a7") sysno,
            in("a0") arg0,
            in("a1") arg1,
            in("a2") arg2,
            in("a3") arg3,
            in("a4") arg4,
            in("a5") arg5,
            lateout("a0") ret,
        );
    }
    ret
}

pub unsafe fn syscall(
    sysno: u64,
    arg0: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
) -> Result<u64, Errno> {
    let res = unsafe { syscall_raw(sysno, arg0, arg1, arg2, arg3, arg4, arg5) };
    let res = res as i64;
    if res < 0 {
        Err(-res as i32)
    } else {
        Ok(res as u64)
    }
}

/// One primary objective of Anemone is to provide solid compatibility with
/// Linux syscalls. Therefore, we define Linux syscall numbers here for
/// reference.
pub mod linux {
    pub const SYS_GETCWD: u64 = 17;

    pub const SYS_DUP: u64 = 23;
    pub const SYS_DUP3: u64 = 24;

    pub const SYS_MKDIRAT: u64 = 34;
    pub const SYS_UNLINKAT: u64 = 35;

    pub const SYS_UMOUNT2: u64 = 39;
    pub const SYS_MOUNT: u64 = 40;

    pub const SYS_CHDIR: u64 = 49;
    pub const SYS_CHROOT: u64 = 51;

    pub const SYS_OPENAT: u64 = 56;
    pub const SYS_CLOSE: u64 = 57;
    pub const SYS_PIPE2: u64 = 59;

    pub const SYS_GETDENTS64: u64 = 61;

    pub const SYS_READ: u64 = 63;
    pub const SYS_WRITE: u64 = 64;

    pub const SYS_FSTAT: u64 = 80;

    pub const SYS_EXIT: u64 = 93;

    pub const SYS_NANOSLEEP: u64 = 101;

    pub const SYS_SCHED_YIELD: u64 = 124;

    pub const SYS_TIMES: u64 = 153;

    pub const SYS_UNAME: u64 = 160;

    pub const SYS_GETTIMEOFDAY: u64 = 169;

    pub const SYS_GETPID: u64 = 172;
    pub const SYS_GETPPID: u64 = 173;

    pub const SYS_BRK: u64 = 214;

    pub const SYS_CLONE: u64 = 220;
    pub const SYS_EXECVE: u64 = 221;

    pub const SYS_WAIT4: u64 = 260;
}

pub use linux::*;

use crate::errno::Errno;
