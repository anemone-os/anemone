mod api;

pub const SYSNAME: &[u8] = b"Anemone\x00";
pub const NODENAME: &[u8] = b"anemone\x00";
/// Fake kernel version for compatibility. Some system softwares (e.g. ld.so)
/// rely on this.
///
/// This is an ugly hack. We'd better patch those softwares to make them run
/// without kernel modification, but it's not a priority for now. (also a bit
/// tricky...)
pub const RELEASE: &[u8] = b"6.6.32\x00";
/// The same as [RELEASE].
pub const VERSION: &[u8] = b"6.6.32\x00";

#[cfg(target_arch = "riscv64")]
pub const MACHINE: &[u8] = b"riscv64\x00";
#[cfg(target_arch = "loongarch64")]
pub const MACHINE: &[u8] = b"loongarch64\x00";
