use fdt::nodes::{AsNode, cpus::CpuStatus};

use crate::{
    device::{finish_cpu_registration, register_cpu},
    prelude::*,
};

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

static MINIMUM_REQUIRED_ISA_EXTENSIONS: &[&str] = &["i", "m", "a", "c", "f", "d"];

/// Get the list of ISA extensions from the `riscv,isa` property of a CPU node
/// in the device tree. Returns `None` if the property is missing or malformed.
///
/// A valid format for the `riscv,isa` property is
/// `rv64<single-letter-extensions>[_<multi-letter-extensions>]+`, e.g.
/// `rv64imafdc_zicsr_zifencei`.
fn parse_riscv_isa(isa: &str) -> Option<Vec<&str>> {
    let mut parts = isa.strip_prefix("rv64")?.split('_');
    let single_letter = parts.next()?;
    if single_letter.is_empty() || !single_letter.bytes().all(|byte| byte.is_ascii_lowercase()) {
        return None;
    }

    let mut extensions = Vec::with_capacity(single_letter.len());
    for index in 0..single_letter.len() {
        let extension = &single_letter[index..index + 1];
        if extension == "g" {
            extensions.extend(["i", "m", "a", "f", "d", "zicsr", "zifencei"]);
        } else {
            extensions.push(extension);
        }
    }

    for extension in parts {
        if extension.len() < 2
            || !extension
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        {
            return None;
        }
        extensions.push(extension);
    }

    Some(extensions)
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
        if validate_cpu_node(cpu) {
            register_cpu(PhysCpuId::new(cpuid as usize));
            ncpus += 1;
        }
    }
    finish_cpu_registration();
    ncpus
}

fn validate_cpu_node(cpu_node: fdt::nodes::cpus::Cpu) -> bool {
    // step 1: get cpu id
    let Ok(cpuid) = cpu_node.clone().reg::<u32>().first() else {
        kwarningln!(
            "failed to find cpu id for cpu node #? in device tree. Assuming it is not available."
        );
        return false;
    };
    // step 2: check cpu status
    match cpu_node.status() {
        Some(CpuStatus::OKAY) => {},
        Some(_) => {
            return false;
        },
        None => {
            kwarningln!(
                "no status property found for cpu #{:}. Assuming it is available.",
                cpuid
            );
        },
    };
    // step 3: check cpu isa extensions
    let Some(isa_property) = cpu_node.as_node().properties().find("riscv,isa") else {
        kwarningln!(
            "no riscv,isa property found for cpu #{:}. Assuming it is not available.",
            cpuid
        );
        return false;
    };
    let Ok(isa) = isa_property.as_value::<&str>() else {
        kwarningln!(
            "failed to parse riscv,isa property for cpu #{:}. Assuming it is not available.",
            cpuid
        );
        return false;
    };
    let Some(ext_list) = parse_riscv_isa(isa) else {
        kwarningln!(
            "cpu #{:} has malformed riscv,isa property '{:}'. Assuming it is not available.",
            cpuid,
            isa
        );
        return false;
    };
    for &required_ext in MINIMUM_REQUIRED_ISA_EXTENSIONS {
        if !ext_list.contains(&required_ext) {
            kwarningln!(
                "cpu #{:} does not have required ISA extension '{:}'. A minimum requirement of {:} is required. Assuming it is not available.",
                cpuid,
                required_ext,
                MINIMUM_REQUIRED_ISA_EXTENSIONS.join(", ")
            );
            return false;
        }
    }
    // step 4: check mmu-type property
    // currently we support sv39 only, so we can ignore the value of mmu-type
    // property, but we still need to check if it exists.
    if cpu_node.as_node().properties().find("mmu-type").is_none() {
        kwarningln!(
            "no mmu-type property found for cpu #{:}. Assuming it is not available.",
            cpuid
        );
        return false;
    }
    true
}
