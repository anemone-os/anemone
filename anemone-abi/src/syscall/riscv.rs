//! System call conventions and numbers.
//! Architecture-specific.

pub unsafe fn syscall(
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

/// One primary objective of Anemone is to provide solid compatibility with
/// Linux syscalls. Therefore, we define Linux syscall numbers here for
/// reference.
pub mod linux {
    pub const SYS_READ: u64 = 63;
    pub const SYS_WRITE: u64 = 64;
    pub const SYS_OPENAT: u64 = 56;
    pub const SYS_CLOSE: u64 = 57;
}

pub use linux::*;
