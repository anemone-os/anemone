use crate::device::CpuArchTrait;

/// LoongArch64 CPU-specific architecture hooks.
pub struct La64CpuArch;
impl CpuArchTrait for La64CpuArch {
    /// Set the per-CPU base register used by the current core.
    unsafe fn set_percpu_base(base: *mut u8) {
        unsafe {
            core::arch::asm!("move $tp, {}", in(reg) base as usize);
        }
    }

    /// Read the current per-CPU base register.
    fn percpu_base() -> usize {
        let base: usize;
        unsafe {
            core::arch::asm!("move {}, $tp", out(reg) base);
        }
        base
    }
}
