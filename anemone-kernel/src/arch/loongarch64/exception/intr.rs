use la_insc::reg::crmd;

use crate::prelude::*;

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
        todo!()
    }
}
