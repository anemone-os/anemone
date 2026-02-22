/// For RiscV, `IrqFlags` stores the value of SIE bit in SSTATUS csr, which
/// indicates whether interrupts are enabled or not.
mod intr;
pub use intr::RiscV64Exception as Exception;
mod trap;
pub use trap::{RiscV64Trap as Trap, use_ktrap_entry};
