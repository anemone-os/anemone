//! Loongarch64 instruction support.

//#![deny(missing_docs)]
#![no_std]
#[cfg(target_arch = "loongarch64")]
pub mod utils;
#[cfg(target_arch = "loongarch64")]
pub mod reg;
#[cfg(target_arch = "loongarch64")]
pub mod insc;