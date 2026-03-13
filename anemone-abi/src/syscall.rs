//! System call conventions and numbers.
//! Architecture-specific.

/// The Linux kernel actually does not define a maximum syscall number,
/// but it's obvious that syscall numbers won't exceed this value, on any
/// architecture.
///
/// Anemone defines its own syscall number starting from this value.
pub const LINUX_SYSNO_MAX: u64 = 0x200;

/// Anemone-native syscall numbers.
pub mod native {
    pub use super::*;
    /// The syscall number where Anemone-specific syscalls start.
    ///
    /// Currently unused.
    pub const SYS_ANEMONE_START: u64 = LINUX_SYSNO_MAX + 0;
}
pub use native::*;

#[cfg(target_arch = "riscv64")]
pub use riscv64::*;
#[cfg(target_arch = "riscv64")]
pub mod riscv64 {
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
}

/// One primary objective of Anemone is to provide solid compatibility with
/// Linux syscalls. Therefore, we define Linux syscall numbers here for
/// reference.
pub mod linux {
    #[cfg(target_arch = "riscv64")]
    pub mod riscv64 {
        pub const SYS_READ: u64 = 63;
        pub const SYS_WRITE: u64 = 64;
        pub const SYS_OPENAT: u64 = 56;
        pub const SYS_CLOSE: u64 = 57;
        // TODO: Add more syscall numbers as needed.
    }
    #[cfg(target_arch = "riscv64")]
    pub use riscv64::*;
}

pub use linux::*;
