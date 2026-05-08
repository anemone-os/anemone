//! LoongArch64 architecture support.

pub(super) mod cpu;
pub(super) mod exception;

mod backtrace;
mod bootstrap;
mod machine;
pub(super) mod mm;
mod sched;
pub(super) mod time;

pub use backtrace::LA64BacktraceArch as BacktraceArch;
pub use cpu::La64CpuArch as CpuArch;
pub use exception::{
    LA64IntrArch as IntrArch, LA64SignalArch as SignalArch, LA64TrapArch as TrapArch,
};
pub use machine::machine_init;
pub use mm::{LA64KernelLayout as KernelLayout, LA64PagingArch as PagingArch};
pub use sched::LA64SchedArch as SchedArch;
pub use time::LA64TimeArch as TimeArch;
