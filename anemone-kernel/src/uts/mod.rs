mod api;

pub const SYSNAME: &[u8] = b"Anemone\x00";
pub const NODENAME: &[u8] = b"anemone\x00";
pub const RELEASE: &[u8] = b"0.1\x00";
pub const VERSION: &[u8] = b"0.1\x00";

#[cfg(target_arch = "riscv64")]
pub const MACHINE: &[u8] = b"riscv64\x00";
#[cfg(target_arch = "loongarch64")]
pub const MACHINE: &[u8] = b"loongarch64\x00";
