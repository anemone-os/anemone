pub mod intr;
pub mod trap;

pub use intr::LA64IntrArch;
pub use trap::LA64TrapArch;
pub use trap::on_enter_kernel;