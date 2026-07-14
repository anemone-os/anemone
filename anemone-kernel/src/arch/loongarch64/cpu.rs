use fdt::nodes::cpus::CpuStatus;

use crate::{
    device::{finish_cpu_registration, register_cpu},
    prelude::*,
};

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

/// Scan the CPU count from the device tree.
///
/// Mostly used for waking up APs in SMP initialization.
///
/// # Safety
///
/// - Caller must ensure that the provided `fdt` is valid.
pub unsafe fn early_scan_cpu_count(fdt: VirtAddr) -> usize {
    let fdt = unsafe { fdt::Fdt::from_ptr(fdt.as_ptr()) }.expect("failed to parse device tree");

    let mut ncpus = 0;

    for cpu in fdt.root().cpus().iter() {
        let cpuid = cpu.clone().reg::<u32>().first().unwrap_or_else(|e| {
            panic!("error finding cpu id for cpu in slot #{:}: {:?}", ncpus, e)
        });

        match cpu.status() {
            Some(CpuStatus::OKAY) => {},
            Some(_) => panic!(
                "unsupported CPU status of cpu #{:}: {:?}",
                cpuid,
                cpu.status()
            ),
            None => {
                kwarningln!("no status property found for cpu #{:}.", cpuid);
            },
        }

        register_cpu(PhysCpuId::new(cpuid as usize));
        ncpus += 1;
    }

    finish_cpu_registration();
    ncpus
}
