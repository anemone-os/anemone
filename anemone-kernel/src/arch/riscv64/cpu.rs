use crate::prelude::*;

pub struct RiscV64Cpu;

pub use RiscV64Cpu as Cpu;

static mut NCPUS: usize = 0;

pub(super) unsafe fn set_ncpus(ncpus: usize) {
    unsafe {
        NCPUS = ncpus;
    }
}

impl CpuArch for RiscV64Cpu {
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

    fn cur_cpu_id() -> usize {
        with_core_local(|core_local| core_local.cpu_id())
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
