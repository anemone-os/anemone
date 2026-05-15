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

    pub const SYS_FCNTL: u64 = 25;

    pub const SYS_IOCTL: u64 = 29;

    pub const SYS_MKDIRAT: u64 = 34;
    pub const SYS_UNLINKAT: u64 = 35;
    pub const SYS_SYMLINKAT: u64 = 36;

    pub const SYS_UMOUNT2: u64 = 39;
    pub const SYS_MOUNT: u64 = 40;

    pub const SYS_STATFS: u64 = 43;

    pub const SYS_FACCESSAT: u64 = 48;

    pub const SYS_CHDIR: u64 = 49;
    pub const SYS_CHROOT: u64 = 51;

    pub const SYS_OPENAT: u64 = 56;
    pub const SYS_CLOSE: u64 = 57;
    pub const SYS_PIPE2: u64 = 59;

    pub const SYS_GETDENTS64: u64 = 61;

    pub const SYS_READ: u64 = 63;
    pub const SYS_WRITE: u64 = 64;
    pub const SYS_WRITEV: u64 = 66;

    pub const SYS_SENDFILE: u64 = 71;
    pub const SYS_PPOLL: u64 = 73;

    pub const SYS_READLINKAT: u64 = 78;
    pub const SYS_NEWFSTATAT: u64 = 79;
    pub const SYS_FSTAT: u64 = 80;

    pub const SYS_UTIMENSAT: u64 = 88;

    pub const SYS_EXIT: u64 = 93;
    pub const SYS_EXIT_GROUP: u64 = 94;
    pub const SYS_SET_TID_ADDRESS: u64 = 96;
    pub const SYS_SET_ROBUST_LIST: u64 = 99;

    pub const SYS_NANOSLEEP: u64 = 101;
    pub const SYS_CLOCK_GETTIME: u64 = 113;
    pub const SYS_CLOCK_GETRES: u64 = 114;

    pub const SYS_SYSLOG: u64 = 116;

    pub const SYS_SCHED_YIELD: u64 = 124;

    pub const SYS_KILL: u64 = 129;
    pub const SYS_TGKILL: u64 = 131;
    pub const SYS_SIGALTSTACK: u64 = 132;
    pub const SYS_RT_SIGACTION: u64 = 134;
    pub const SYS_RT_SIGPROCMASK: u64 = 135;
    pub const SYS_RT_SIGPENDING: u64 = 136;
    pub const SYS_RT_SIGTIMEDWAIT: u64 = 137;
    pub const SYS_RT_SIGQUEUEINFO: u64 = 138;
    pub const SYS_RT_SIGRETURN: u64 = 139;

    pub const SYS_SETGID: u64 = 144;
    pub const SYS_SETUID: u64 = 146;

    pub const SYS_TIMES: u64 = 153;

    pub const SYS_UNAME: u64 = 160;

    pub const SYS_GETRLIMIT: u64 = 163;
    pub const SYS_GETRUSAGE: u64 = 165;

    pub const SYS_GETTIMEOFDAY: u64 = 169;

    pub const SYS_GETPID: u64 = 172;
    pub const SYS_GETPPID: u64 = 173;
    pub const SYS_GETUID: u64 = 174;
    pub const SYS_GETGID: u64 = 176;
    pub const SYS_GETTID: u64 = 178;

    pub const SYS_SYSINFO: u64 = 179;

    pub const SYS_BRK: u64 = 214;
    pub const SYS_MUNMAP: u64 = 215;

    pub const SYS_CLONE: u64 = 220;
    pub const SYS_EXECVE: u64 = 221;
    pub const SYS_MMAP: u64 = 222;
    pub const SYS_MPROTECT: u64 = 226;
    pub const SYS_MADVISE: u64 = 233;

    pub const SYS_WAIT4: u64 = 260;

    pub const SYS_PRLIMIT64: u64 = 261;

    pub const SYS_RENAMEAT2: u64 = 276;

    pub const SYS_GETRANDOM: u64 = 278;

    pub const SYS_CLONE3: u64 = 435;

    pub const SYS_FACCESSAT2: u64 = 439;
}

pub use linux::*;

use crate::errno::Errno;
