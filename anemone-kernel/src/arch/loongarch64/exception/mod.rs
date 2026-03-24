pub mod intr;
pub mod trap;

pub use intr::LA64IntrArch;
use la_insc::reg::{
    crmd,
    csr::ecfg,
    exception::{Ecfg, IntrFlags},
};
pub use trap::{LA64TrapArch, install_ktrap_handler};

/// Enable local interrupts
pub fn enable_local_irq() {
    kdebugln!("enabling local interrupts...");
    use crate::prelude::*;
    unsafe {
        ecfg::csr_write(Ecfg::new(IntrFlags::all(), 0));
        crmd::set_ie(true);
    }
}
