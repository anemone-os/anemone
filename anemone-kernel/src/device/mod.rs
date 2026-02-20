//! Device module, which contains code for initializing and managing devices.

pub mod discovery;

mod cpu;
pub use cpu::CpuArch;
mod serial;

pub trait PlatformDevice {}
