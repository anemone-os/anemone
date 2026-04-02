use crate::{arch::riscv64::exception::trap::RiscV64Interrupt, prelude::*};

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

    unsafe fn claim_ipi() {
        unsafe {
            riscv::register::sip::set_ssoft();
        }
    }

    unsafe fn init_local_irq() {
        use crate::prelude::*;

        unsafe {
            riscv::register::sstatus::set_sie();
            riscv::register::sie::set_ssoft();
            riscv::register::sie::set_stimer();
            riscv::register::sie::set_sext();

            // this fires up system timer
            sbi_rt::set_timer(TimeArch::current_ticks().wrapping_add(300_000_0) as u64)
                .expect("failed to set timer for next timer interrupt");
        }
    }
}

pub unsafe fn handle_intr(reason: RiscV64Interrupt) {
    match reason {
        RiscV64Interrupt::SupervisorSoftware => {
            // claiming after handling will result in missing IPI, leading to queue
            // congestion.
            unsafe {
                riscv::register::sip::clear_ssoft();
            }
            handle_ipi();
        },
        RiscV64Interrupt::SupervisorTimer => {
            // TODO: use a proper value for the next timer interrupt.
            sbi_rt::set_timer(riscv::register::time::read().wrapping_add(300_000_0) as u64)
                .expect("failed to set timer for next timer interrupt");
            handle_kernel_timer_interrupt();
        },
        RiscV64Interrupt::SupervisorExternal => handle_irq(),
    }
}
