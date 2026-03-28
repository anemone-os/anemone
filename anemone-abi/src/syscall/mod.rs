#[cfg(target_arch = "riscv64")]
mod riscv;
#[cfg(target_arch = "riscv64")]
pub use riscv::*;

#[cfg(target_arch = "loongarch64")]
mod loongarch;
#[cfg(target_arch = "loongarch64")]
pub use loongarch::*;

mod native;
pub use native::*;

/// The Linux kernel actually does not define a maximum syscall number,
/// but it's obvious that syscall numbers won't exceed this value minus one, on
/// any architecture.
///
/// Anemone defines its own syscall number starting from this value.
pub const LINUX_SYSNO_MAX: u64 = 0x200;

/// The syscall number where Anemone-specific syscalls start.
///
/// Currently unused.
pub const SYS_ANEMONE_START: u64 = LINUX_SYSNO_MAX + 0;

/// Syscall (Linux or Anemone-native) numbers will not exceed this value minus
/// one.
pub const ANEMONE_SYSNO_MAX: u64 = 0x400;
