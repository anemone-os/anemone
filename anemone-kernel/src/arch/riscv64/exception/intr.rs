use crate::prelude::*;

pub struct RiscV64IntrArch;

trait AsSie {
    fn from_sie(sie: bool) -> Self;
    fn to_sie(self) -> bool;
}

impl AsSie for IrqFlags {
    fn from_sie(sie: bool) -> Self {
        Self::new(if sie { 1 } else { 0 })
    }

    fn to_sie(self) -> bool {
        #[cfg(debug_assertions)]
        {
            assert!(
                matches!(self.raw(), 0 | 1),
                "Invalid IrqFlags value: {}",
                self.raw()
            );
        }
        self.raw() & 1 != 0
    }
}

impl IntrArchTrait for RiscV64IntrArch {
    const ENABLED_IRQ_FLAGS: IrqFlags = IrqFlags::new(1);
    const DISABLED_IRQ_FLAGS: IrqFlags = IrqFlags::new(0);

    fn current_irq_flags() -> IrqFlags {
        IrqFlags::from_sie(riscv::register::sstatus::read().sie())
    }

    unsafe fn restore_local_intr(flags: IrqFlags) {
        unsafe {
            if flags.to_sie() {
                riscv::register::sstatus::set_sie();
            } else {
                riscv::register::sstatus::clear_sie();
            }
        }
    }

    fn send_ipi(cpu_id: usize) {
        let hartmask = sbi_rt::HartMask::from_mask_base(1 << cpu_id, 0);
        sbi_rt::send_ipi(hartmask).expect("ipi send failed, cannot recover");
    }
}
