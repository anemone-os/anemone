//! This module is used to resolve platform configurations
//! under the `conf/platforms/` directory.

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug, Serialize)]
pub enum Arch {
    #[serde(rename = "riscv64")]
    RiscV64,
    #[serde(rename = "loongarch64")]
    LoongArch64,
}

impl Arch {
    pub fn as_str(&self) -> &'static str {
        match self {
            Arch::RiscV64 => "riscv64",
            Arch::LoongArch64 => "loongarch64",
        }
    }
}

#[derive(Deserialize, Debug, Serialize)]
pub enum ExecEnv {
    #[serde(rename = "sbi")]
    Sbi,
    #[serde(rename = "uefi")]
    Uefi,
}

#[derive(Deserialize, Debug, Serialize)]
pub enum TargetTriple {
    #[serde(rename = "riscv64-unknown-anemone-elf")]
    RiscV64UnknownAnemoneElf,
    #[serde(rename = "loongarch64-unknown-anemone-elf")]
    LoongArch64UnknownAnemoneElf,
}

impl TargetTriple {
    pub fn as_str(&self) -> &'static str {
        match self {
            TargetTriple::RiscV64UnknownAnemoneElf => "riscv64-unknown-anemone-elf",
            TargetTriple::LoongArch64UnknownAnemoneElf => "loongarch64-unknown-anemone-elf",
        }
    }
}

#[derive(Deserialize, Debug, Serialize)]
pub struct Build {
    pub name: String,
    pub abbrs: Vec<String>,
    pub arch: Arch,
    pub target: TargetTriple,
    pub exec_env: ExecEnv,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct Constants {
    pub phys_ram_start: u64,
    pub max_phys_ram_size: u64,
    pub kernel_la_base: u64,
    pub kernel_va_base: u64,
    pub max_user_stack_size: u64,
    pub init_user_stack_size: u64,
    pub max_heap_size: u64,
    pub max_cpus: usize,
    pub frame_section_shift_mb: usize,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct Qemu {
    pub qemu: String,
    pub machine: String,
    pub cpu: Option<String>,
    pub smp: u64,
    pub memory: String,
    pub bios: Option<String>,
    pub args: Option<Vec<String>>,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct Dtb {
    pub path: String,
    #[serde(rename = "type")]
    pub typ: DtbType,
    pub source: Option<String>,
}

#[derive(Deserialize, Debug, Serialize)]
pub enum DtbType {
    #[serde(rename = "qemu")]
    Qemu,
    #[serde(rename = "file")]
    File,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct Config {
    pub build: Build,
    pub constants: Constants,
    pub qemu: Option<Qemu>,
    pub dtb: Option<Dtb>,
}

impl Config {
    pub fn from_str(content: &str) -> anyhow::Result<Self> {
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
    pub fn gen_platform_defs(&self) -> String {
        format!(
            r#"//! Auto-generated platform constants, do not edit manually.
#![allow(unused)]

/// Physical RAM start address
pub const PHYS_RAM_START: u64 = {:#x};
/// Maximum physical RAM size supported by this platform
pub const MAX_PHYS_RAM_SIZE: u64 = {:#x};
/// Kernel load address
pub const KERNEL_LA_BASE: u64 = {:#x};
/// Kernel virtual address base
pub const KERNEL_VA_BASE: u64 = {:#x};
/// Maximum user stack size
pub const MAX_USER_STACK_SIZE: u64 = {:#x};
/// Initial user stack size
pub const INIT_USER_STACK_SIZE: u64 = {:#x};
/// Maximum heap size
pub const MAX_HEAP_SIZE: u64 = {:#x};
/// Maximum number of CPUs supported
pub const MAX_CPUS: usize = {};
/// Frame section size shift in megabytes
pub const FRAME_SECTION_SHIFT_MB: usize = {};

        "#,
            self.constants.phys_ram_start,
            self.constants.max_phys_ram_size,
            self.constants.kernel_la_base,
            self.constants.kernel_va_base,
            self.constants.max_user_stack_size,
            self.constants.init_user_stack_size,
            self.constants.max_heap_size,
            self.constants.max_cpus,
            self.constants.frame_section_shift_mb,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parsing() {
        let content = std::fs::read_to_string("../../conf/platforms/qemu-virt-rv64.toml").unwrap();
        let config = Config::from_str(&content).unwrap();
        println!("{:#x?}", config);
    }
}
