//! LoongArch64 architecture support.

pub(super) mod cpu;
pub(super) mod exception;

pub(super) mod mm;
pub(super) mod time;
mod sched;
mod backtrace;
mod machine;
mod bootstrap;
mod utils;

pub use backtrace::LA64BacktraceArch as BacktraceArch;
pub use cpu::La64CpuArch as CpuArch;
pub use exception::{LA64IntrArch as IntrArch, LA64TrapArch as TrapArch};
pub use mm::{LA64KernelLayout as KernelLayout, LA64PagingArch as PagingArch};
pub use time::LA64TimeArch as TimeArch;
pub use sched::LA64SchedArch as SchedArch;
pub use machine::machine_init;
