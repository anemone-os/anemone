//! Clock sources are used to provide a timebase for the kernel. They are used
//! by the scheduler to determine when to switch tasks, and by the timer
//! interrupt handler to determine how long to wait before the next interrupt.

#[cfg(target_arch = "riscv64")]
pub mod sbi_timer;
