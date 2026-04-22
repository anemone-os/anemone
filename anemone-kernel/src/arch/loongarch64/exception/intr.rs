use la_insc::reg::{
    crmd,
    csr::ecfg,
    exception::{Ecfg, IntrFlags},
};
use loongArch64::{iocsr::iocsr_write_w, ipi::send_ipi_single};

use crate::{arch::loongarch64::exception::trap::LA64Interrupt, prelude::*};

pub struct LA64IntrArch;
impl IntrArchTrait for LA64IntrArch {
    const ENABLED_IRQ_FLAGS: IrqFlags = IrqFlags::new(1);

    const DISABLED_IRQ_FLAGS: IrqFlags = IrqFlags::new(0);

    /// Read the current local interrupt state from `CRMD.IE`.
    fn current_irq_flags() -> IrqFlags {
        if crmd::read_ie() {
            Self::ENABLED_IRQ_FLAGS
        } else {
            Self::DISABLED_IRQ_FLAGS
        }
    }

    /// Restore the local interrupt enable bit from saved flags.
    unsafe fn restore_local_intr(flags: IrqFlags) {
        if flags == Self::DISABLED_IRQ_FLAGS {
            crmd::set_ie(false);
        } else {
            crmd::set_ie(true);
        }
    }

    /// Send an inter-processor interrupt to the target CPU.
    fn send_ipi(cpu_id: usize) {
        unsafe {
            send_ipi_single(cpu_id, 1);
        }
    }

    /// Claim a pending IPI by clearing the platform IOCSR state.
    unsafe fn claim_ipi() {
        unsafe {
            iocsr_write_w(0x100c, u32::MAX);
        }
    }

    /// Enable local interrupts and unmask platform interrupt sources.
    unsafe fn init_local_irq() {
        unsafe {
            ecfg::csr_write(Ecfg::new(IntrFlags::all(), 0));
            iocsr_write_w(0x1004, u32::MAX);
            crmd::set_ie(true);
            knoticeln!("({})local irq initialized", cur_cpu_id());
        }
    }
}

/// Dispatch a decoded interrupt reason to the appropriate handler.
pub(super) unsafe fn handle_intr(reason: LA64Interrupt) {
    match reason {
        LA64Interrupt::Timer => {
            TimeArch::claim_timer_interrupt();
            handle_timer_interrupt();
        },
        LA64Interrupt::Ipi => {
            // claiming after handling will result in missing IPI, leading to queue
            // congestion.
            unsafe {
                IntrArch::claim_ipi();
            }
            handle_ipi();
        },
        LA64Interrupt::Hardware => handle_irq(),
    }
}
