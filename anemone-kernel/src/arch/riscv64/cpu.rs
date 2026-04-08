use crate::prelude::*;

pub struct RiscV64CpuArch;

static mut NCPUS: usize = 0;

static mut BSP_CPU_ID: Option<CpuId> = None;

pub unsafe fn init(ncpus: usize, bsp_cpu_id: usize) {
    unsafe {
        NCPUS = ncpus;
        BSP_CPU_ID = Some(CpuId::new(bsp_cpu_id));
    }
}

impl CpuArchTrait for RiscV64CpuArch {
    fn bsp_cpu_id() -> CpuId {
        unsafe { BSP_CPU_ID.expect("BSP CPU ID not set") }
    }

    fn ncpus() -> usize {
        let ncpus = unsafe { NCPUS };
        #[cfg(debug_assertions)]
        {
            if ncpus == 0 {
                panic!("NCPUS is not set yet");
            }
        }
        ncpus
    }

    fn cur_cpu_id() -> CpuId {
        CpuId::new(unsafe_with_core_local(|core_local| core_local.cpu_id()))
    }

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
