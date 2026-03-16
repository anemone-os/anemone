/// For RiscV, `IrqFlags` stores the value of SIE bit in SSTATUS csr, which
/// indicates whether interrupts are enabled or not.
mod intr;
pub use intr::RiscV64IntrArch;
pub use trap::RiscV64TrapArch;
mod trap;
pub use trap::install_ktrap_handler;

pub fn enable_local_irq() {
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
