pub mod intr;
pub mod trap;

pub use intr::LA64IntrArch;
use la_insc::reg::{
    crmd,
    csr::{ecfg, tcfg, tid},
    exception::{Ecfg, IntrFlags}, iocsr::ipi_enable, timer::Tcfg,
};
pub use trap::{LA64TrapArch, install_ktrap_handler};

use crate::{arch::{CpuArch, TimeArch}, device::CpuArchTrait, time::TimeArchTrait};


/// Enable local interrupts
pub fn enable_local_irq() {
    kdebugln!("enabling local interrupts...");
    use crate::prelude::*;
    unsafe {
        ecfg::csr_write(Ecfg::new(IntrFlags::all(), 0));
        // only enable ipi 0
        ipi_enable::io_csr_write(1 << 0);
        crmd::set_ie(true);
    }
}
