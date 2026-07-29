pub mod intr;
pub mod trap;
#[cfg(feature = "soft_unaligned_access")]
mod unaligned;

pub use intr::LA64IntrArch;
pub use trap::{LA64SignalArch, LA64TrapArch, install_ktrap_handler};
