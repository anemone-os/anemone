//! This module is used to resolve platform configurations
//! under the `conf/platforms/` directory.

use std::{
    collections::HashSet,
    path::{Component, Path},
};

use serde::{Deserialize, Serialize};

use super::{reference::validate_slug, system_target::Root};

#[derive(Deserialize, Debug, Serialize, Clone)]
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

    pub fn target_triple(&self) -> TargetTriple {
        match self {
            Arch::RiscV64 => TargetTriple::RiscV64UnknownAnemoneElf,
            Arch::LoongArch64 => TargetTriple::LoongArch64UnknownAnemoneElf,
        }
    }

    pub fn try_from_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "riscv64" => Ok(Arch::RiscV64),
            "loongarch64" => Ok(Arch::LoongArch64),
            _ => anyhow::bail!("Unsupported architecture: {}", s),
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

#[derive(Debug, Clone, Copy)]
pub enum TargetTriple {
    RiscV64UnknownAnemoneElf,
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
    pub exec_env: ExecEnv,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct Constants {
    pub phys_ram_start: u64,
    pub max_phys_ram_size: u64,
    pub kernel_la_base: u64,
    pub kernel_va_base: u64,
    pub max_phys_cpu_id: usize,
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
    #[serde(default)]
    pub bind: Vec<QemuBind>,
}

#[derive(Deserialize, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QemuBind {
    pub name: String,
    pub template: Vec<String>,
}

#[derive(Deserialize, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Dtb {
    pub source: String,
    pub delivery: DtbDelivery,
    pub authority: DtsAuthority,
    pub provider: Option<DtbProvider>,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct Uboot {
    pub arch: String,
    pub os_type: String,
    pub image_type: String,
    pub compression: String,
    pub load_addr: u64,
    pub entry: u64,
    pub name: String,
    pub filename: String,
}

#[derive(Deserialize, Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DtbDelivery {
    Firmware,
    Embedded,
}

#[derive(Deserialize, Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DtsAuthority {
    ProviderDerived,
    Normative,
}

#[derive(Deserialize, Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DtbProvider {
    Qemu,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct Config {
    pub build: Build,
    pub constants: Constants,
    pub qemu: Option<Qemu>,
    pub dtb: Option<Dtb>,
    pub uboot: Option<Uboot>,
}

impl Config {
    pub fn from_str(content: &str) -> anyhow::Result<Self> {
        let config: Config = toml::from_str(&content)?;
        if let Some(dtb) = &config.dtb {
            dtb.validate()?;
        }
        if let Some(qemu) = &config.qemu {
            validate_qemu_bindings(&qemu.bind)?;
        }
        Ok(config)
    }
    pub fn gen_platform_defs(&self, root: &Root) -> String {
        let rootfs_source_path = root
            .source
            .path()
            .map(|path| format!("Some({path:?})"))
            .unwrap_or_else(|| "None".to_string());

        format!(
            r#"//! Auto-generated platform constants, do not edit manually.
#![allow(unused)]

use crate::mm::addr::{{PhysAddr, VirtAddr}};

/// Physical RAM start address
pub const PHYS_RAM_START: PhysAddr = PhysAddr::new({:#x});
/// Maximum physical RAM size supported by this platform
pub const MAX_PHYS_RAM_SIZE: u64 = {:#x};
/// Kernel load address
pub const KERNEL_LA_BASE: PhysAddr = PhysAddr::new({:#x});
/// Kernel virtual address base
pub const KERNEL_VA_BASE: VirtAddr = VirtAddr::new({:#x});
/// Inclusive upper bound of firmware-visible physical CPU IDs
pub const MAX_PHYS_CPU_ID: usize = {};
/// Frame section size shift in megabytes
pub const FRAME_SECTION_SHIFT_MB: usize = {};
/// Root filesystem type
pub const ROOTFS_FS_TYPE: &str = {:?};
/// Root filesystem source kind
pub const ROOTFS_SOURCE_KIND: &str = {:?};
/// Root filesystem source path
pub const ROOTFS_SOURCE_PATH: Option<&str> = {};

        "#,
            self.constants.phys_ram_start,
            self.constants.max_phys_ram_size,
            self.constants.kernel_la_base,
            self.constants.kernel_va_base,
            self.constants.max_phys_cpu_id,
            self.constants.frame_section_shift_mb,
            root.fstype,
            root.source.kind(),
            rootfs_source_path,
        )
    }
}

pub fn validate_qemu_bindings(bindings: &[QemuBind]) -> anyhow::Result<()> {
    let mut names = HashSet::new();
    for binding in bindings {
        validate_slug("QEMU bind", &binding.name)?;
        if !names.insert(binding.name.as_str()) {
            anyhow::bail!("duplicate QEMU bind `{}`", binding.name);
        }
        let mut placeholders = 0;
        for token in &binding.template {
            placeholders += token.matches("{{}}").count();
            let literal = token.replace("{{}}", "");
            if literal.contains("{{") || literal.contains("}}") {
                anyhow::bail!(
                    "QEMU bind `{}` contains an unsupported placeholder",
                    binding.name
                );
            }
        }
        if placeholders == 0 {
            anyhow::bail!("QEMU bind `{}` has no `{{{{}}}}` placeholder", binding.name);
        }
    }
    Ok(())
}

impl Dtb {
    fn validate(&self) -> anyhow::Result<()> {
        let source = Path::new(&self.source);
        if self.source.is_empty()
            || source.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            anyhow::bail!("DT source must be a workspace-relative path");
        }
        match (self.delivery, self.authority, self.provider) {
            (DtbDelivery::Firmware, DtsAuthority::ProviderDerived, Some(DtbProvider::Qemu))
            | (DtbDelivery::Embedded, DtsAuthority::Normative, None) => Ok(()),
            _ => anyhow::bail!(
                "invalid DT contract: firmware delivery requires provider-derived authority and provider=qemu; embedded delivery requires normative authority and no provider"
            ),
        }
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

    #[test]
    fn rejects_unsupported_architecture() {
        let content = std::fs::read_to_string("../../conf/platforms/qemu-virt-rv64.toml")
            .unwrap()
            .replace("arch = \"riscv64\"", "arch = \"x86_64\"");
        assert!(Config::from_str(&content).is_err());
    }

    #[test]
    fn accepts_supported_dtb_contracts() {
        let firmware = std::fs::read_to_string("../../conf/platforms/qemu-virt-rv64.toml").unwrap();
        let embedded = std::fs::read_to_string("../../conf/platforms/qemu-virt-la64.toml").unwrap();

        assert!(Config::from_str(&firmware).is_ok());
        assert!(Config::from_str(&embedded).is_ok());
    }

    #[test]
    fn rejects_incoherent_dtb_contracts() {
        let valid = std::fs::read_to_string("../../conf/platforms/qemu-virt-rv64.toml").unwrap();

        for invalid in [
            valid.replace("provider = \"qemu\"\n", ""),
            valid.replace("delivery = \"firmware\"", "delivery = \"embedded\""),
            valid.replace(
                "authority = \"provider-derived\"",
                "authority = \"normative\"",
            ),
            valid.replace("provider = \"qemu\"", "provider = \"other\""),
            valid.replace(
                "source = \"conf/platforms/qemu-virt-rv64.dts\"",
                "source = \"\"",
            ),
            valid.replace(
                "source = \"conf/platforms/qemu-virt-rv64.dts\"",
                "source = \"../qemu-virt-rv64.dts\"",
            ),
        ] {
            assert!(Config::from_str(&invalid).is_err(), "{invalid}");
        }

        let embedded = std::fs::read_to_string("../../conf/platforms/qemu-virt-la64.toml").unwrap();
        assert!(Config::from_str(&format!("{embedded}\nprovider = \"qemu\"\n")).is_err());
    }

    #[test]
    fn validates_dormant_qemu_bind_declarations() {
        let valid = std::fs::read_to_string("../../conf/platforms/qemu-virt-rv64.toml").unwrap()
            + r#"

[[qemu.bind]]
name = "disk-x0"
template = ["-drive", "file={{}},backup={{}},format=raw"]
"#;
        let config = Config::from_str(&valid).unwrap();
        assert_eq!(config.qemu.unwrap().bind.len(), 1);

        for invalid in [
            valid.replace("name = \"disk-x0\"", "name = \"Disk_X0\""),
            valid.replace(
                "template = [\"-drive\", \"file={{}},backup={{}},format=raw\"]",
                "template = [\"-drive\", \"file=fixed,format=raw\"]",
            ),
            valid.replace("file={{}}", "file={{path}}"),
            format!("{valid}\n[[qemu.bind]]\nname = \"disk-x0\"\ntemplate = [\"file={{}}\"]\n"),
            format!("{valid}\nunknown = true\n"),
        ] {
            assert!(Config::from_str(&invalid).is_err(), "{invalid}");
        }
    }
}
