use crate::prelude::*;

pub struct RiscV64CpuArch;

impl CpuArchTrait for RiscV64CpuArch {
    unsafe fn set_percpu_base(base: *mut u8) {
        unsafe {
            core::arch::asm!("mv tp, {}", in(reg) base as usize);
        }
    }

    fn percpu_base() -> usize {
        let base: usize;
        unsafe {
            core::arch::asm!("mv {}, tp", out(reg) base);
        }
        base
    }
}
