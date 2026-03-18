//! Loongarch64 architecture support.

pub(super) mod cpu;
pub(super) mod exception;

pub(super) mod mm;
pub(super) mod time;

mod bootstrap;

pub use cpu::La64CpuArch as CpuArch;
pub use exception::{LA64IntrArch as IntrArch, LA64TrapArch as TrapArch};
pub use mm::{LA64KernelLayout as KernelLayout, LA64PagingArch as PagingArch};
pub use time::LA64TimeArch as TimeArch;
