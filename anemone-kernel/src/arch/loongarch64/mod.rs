//! Loongarch64 architecture support.

pub(super) mod cpu;
pub(super) mod exception;

pub(super) mod mm;
pub(super) mod power;
pub(super) mod time;

mod bootstrap;

pub use cpu::La64CpuArch as CpuArch;
pub use exception::{RiscV64IntrArch as IntrArch, RiscV64TrapArch as TrapArch};
pub use mm::{LA64KernelLayout as KernelLayout, LA64PagingArch as PagingArch};
pub use power::RiscV64PowerArch as PowerArch;
pub use time::RiscV64TimeArch as TimeArch;