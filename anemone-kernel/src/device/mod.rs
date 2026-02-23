//! Device module, which contains code for initializing and managing devices.

pub mod discovery;

mod serial;

mod cpu;
pub use cpu::CpuArchTrait;

pub trait PlatformDevice {}
