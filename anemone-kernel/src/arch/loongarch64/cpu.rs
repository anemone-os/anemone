use crate::{
    device::{CpuArchTrait, CpuId},
    prelude::unsafe_with_core_local,
};

/// Number of CPUs discovered during bootstrap.
static mut NCPUS: usize = 0;

/// Record the number of CPUs discovered from firmware.
pub unsafe fn set_ncpus(ncpus: usize) {
    unsafe {
        NCPUS = ncpus;
    }
}

/// LoongArch64 CPU-specific architecture hooks.
pub struct La64CpuArch;
impl CpuArchTrait for La64CpuArch {
    /// Return the number of CPUs discovered during bootstrap.
    fn ncpus() -> usize {
        unsafe { NCPUS }
    }

    /// Return the current CPU identifier from per-CPU storage.
    fn cur_cpu_id() -> CpuId {
        CpuId::new(unsafe_with_core_local(|core_local| core_local.cpu_id()))
    }

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
