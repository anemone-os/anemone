/// For RiscV, `IrqFlags` stores the value of SIE bit in SSTATUS csr, which
/// indicates whether interrupts are enabled or not.
mod intr;
pub use intr::RiscV64IntrArch;
pub use trap::RiscV64TrapArch;
mod trap;
pub use trap::install_ktrap_handler;
pub use trap::RiscV64TrapFrame;
pub use trap::__ktrap_return_to_task;

