use crate::syscall::SYS_ANEMONE_START;

pub const SYS_DBG_PRINT: u64 = SYS_ANEMONE_START + 0;

pub const SYS_POWER_SHUTDOWN: u64 = SYS_ANEMONE_START + 1;

/// Enable or disable Anemone's trap-exit kernel preemption policy.
///
/// Kernels built without the `kernel_preempt` feature accept this native
/// syscall as a no-op so user-test images do not need to mirror kernel config.
pub const SYS_SET_KERNEL_PREEMPT: u64 = SYS_ANEMONE_START + 2;
