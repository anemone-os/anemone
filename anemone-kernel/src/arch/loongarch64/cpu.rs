use crate::{
    device::{CpuArchTrait, CpuId},
    prelude::unsafe_with_core_local,
};

/// Number of CPUs discovered during bootstrap.
static mut NCPUS: usize = 0;

static mut BSP_CPU_ID: Option<CpuId> = None;

pub unsafe fn init(ncpus: usize, bsp_cpu_id: usize) {
    unsafe {
        NCPUS = ncpus;
        BSP_CPU_ID = Some(CpuId::new(bsp_cpu_id));
    }
}

/// LoongArch64 CPU-specific architecture hooks.
pub struct La64CpuArch;
impl CpuArchTrait for La64CpuArch {
    fn bsp_cpu_id() -> CpuId {
        unsafe { BSP_CPU_ID.expect("BSP CPU ID not set") }
    }

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
