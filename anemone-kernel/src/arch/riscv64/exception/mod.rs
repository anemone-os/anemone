/// For RiscV, `IrqFlags` stores the value of SIE bit in SSTATUS csr, which
/// indicates whether interrupts are enabled or not.
mod intr;
pub use intr::RiscV64IntrArch;
pub use trap::RiscV64TrapArch;
mod trap;
pub use trap::{
    __ktrap_return_to_task, RiscV64SignalArch, RiscV64TrapFrame, install_ktrap_handler,
    utrap_return_to_task,
};
