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
pub unsafe fn early_scan_cpu_count(fdt: VirtAddr, bsp_physical_id: PhysCpuId) -> usize {
    let fdt = unsafe { fdt::Fdt::from_ptr(fdt.as_ptr()) }.expect("failed to parse device tree");

    let mut ignored_cpu_count = 0;

    for (slot, cpu) in fdt.root().cpus().iter().enumerate() {
        let cpuid = cpu.clone().reg::<u32>().first().unwrap_or_else(|e| {
            panic!("error finding cpu id for cpu in slot #{:}: {:?}", slot, e)
        });
        let physical_id = PhysCpuId::new(cpuid as usize);
        if !physical_id.is_within_platform_bound() {
            kwarningln!(
                "ignoring {} from CPU node #{} because it exceeds MAX_PHYS_CPU_ID ({})",
                physical_id,
                slot,
                MAX_PHYS_CPU_ID
            );
            continue;
        }

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

        if unsafe { register_cpu(physical_id, bsp_physical_id) }.is_none() {
            ignored_cpu_count += 1;
        }
    }

    unsafe { finish_cpu_registration(bsp_physical_id, ignored_cpu_count) }
}
