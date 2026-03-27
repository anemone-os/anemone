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

    fn current_irq_flags() -> IrqFlags {
        if crmd::read_ie() {
            Self::ENABLED_IRQ_FLAGS
        } else {
            Self::DISABLED_IRQ_FLAGS
        }
    }

    unsafe fn restore_local_intr(flags: IrqFlags) {
        if flags == Self::DISABLED_IRQ_FLAGS {
            crmd::set_ie(false);
        } else {
            crmd::set_ie(true);
        }
    }

    fn send_ipi(cpu_id: usize) {
        unsafe {
            send_ipi_single(cpu_id, 1);
        }
    }

    unsafe fn claim_ipi() {
        unsafe {
            iocsr_write_w(0x100c, u32::MAX);
        }
    }

    unsafe fn init_local_irq() {
        unsafe {
            ecfg::csr_write(Ecfg::new(IntrFlags::all(), 0));
            crmd::set_ie(true);
            knoticeln!("({})local irq initialized", CpuArch::cur_cpu_id());
        }
    }
}

pub(super) unsafe fn handle_intr(reason: LA64Interrupt) {
    match reason {
        LA64Interrupt::Timer => {
            //kdebugln!("received timer interrupt");
            TimeArch::claim_timer_interrupt();
            TimeArch::set_next_trigger(300_000_0);
        },
        LA64Interrupt::Ipi => {
            handle_ipi();
            unsafe {
                IntrArch::claim_ipi();
            }
        },
        LA64Interrupt::Hardware => handle_irq(),
    }
}
