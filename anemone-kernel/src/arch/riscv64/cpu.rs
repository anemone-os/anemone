use crate::prelude::*;

pub struct RiscV64CpuArch;

static mut NCPUS: usize = 0;

pub unsafe fn set_ncpus(ncpus: usize) {
    unsafe {
        NCPUS = ncpus;
    }
}

impl CpuArchTrait for RiscV64CpuArch {
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
        CpuId::new(with_core_local(|core_local| core_local.cpu_id()))
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
