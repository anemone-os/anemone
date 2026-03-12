use crate::{device::CpuArchTrait, prelude::with_core_local};

static mut NCPUS: usize = 0;

pub unsafe fn set_ncpus(ncpus: usize) {
    unsafe {
        NCPUS = ncpus;
    }
}

pub struct La64CpuArch;
impl CpuArchTrait for La64CpuArch {
    fn ncpus() -> usize {
        unsafe { NCPUS }
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
