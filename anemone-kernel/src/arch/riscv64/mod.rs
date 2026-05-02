// Note:
// Some logic or data structures may be sharable between RiscV32 and RiscV64,
// but for simplicity and clarity, we name them all with "RiscV64" prefix for
// now. We can always refactor them later when RiscV32 support is added.

pub(super) mod cpu;
pub(super) mod exception;

pub(super) mod mm;
pub(super) mod time;

mod backtrace;
mod bootstrap;
mod machine;
mod sched;
// mod trampoline;

pub use backtrace::RiscV64BacktraceArch as BacktraceArch;
pub use cpu::RiscV64CpuArch as CpuArch;
pub use exception::{RiscV64IntrArch as IntrArch, RiscV64TrapArch as TrapArch};
pub use machine::machine_init;
pub use mm::{KernelLayout, RiscV64PagingArch as PagingArch};
pub use sched::RiscV64SchedArch as SchedArch;
pub use time::RiscV64TimeArch as TimeArch;
