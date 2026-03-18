pub mod intr;
pub mod trap;

pub use intr::LA64IntrArch;
pub use trap::{LA64TrapArch, install_ktrap_handler};
