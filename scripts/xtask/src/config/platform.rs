//! This module is used to resolve platform configurations
//! under the `conf/platforms/` directory.

use std::{
    collections::{HashMap, HashSet},
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
#[serde(deny_unknown_fields)]
pub struct Build {
    pub arch: Arch,
    pub exec_env: ExecEnv,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct Constants {
    pub phys_ram_start: u64,
    pub max_phys_ram_size: u64,
    pub kernel_la_base: u64,
    pub kernel_va_base: u64,
    pub earlycon_reg: Option<u64>,
    pub max_phys_cpu_id: usize,
    pub frame_section_shift_mb: usize,
}

#[derive(Deserialize, Debug, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Qemu {
    pub machine: String,
    pub cpu: String,
    pub smp: String,
    pub memory: String,
    pub bios: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub bind: Vec<QemuBind>,
}

#[derive(Deserialize, Debug, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct QemuBind {
    pub name: String,
    pub optional: bool,
    pub template: Vec<String>,
}

#[derive(Deserialize, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Dtb {
    pub source: Option<String>,
    pub delivery: DtbDelivery,
    pub authority: DtAuthority,
    pub provider: Option<DtbProvider>,
}

#[derive(Deserialize, Debug, Serialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Uboot {
    #[serde(rename = "image")]
    Image {
        arch: String,
        os_type: String,
        image_type: String,
        compression: String,
        load_addr: u64,
        entry: u64,
        name: String,
        filename: String,
    },
    #[serde(rename = "raw")]
    Raw { filename: String },
}

impl Uboot {
    pub fn filename(&self) -> &str {
        match self {
            Self::Image { filename, .. } | Self::Raw { filename } => filename,
        }
    }
}

#[derive(Deserialize, Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DtbDelivery {
    Firmware,
    Embedded,
}

#[derive(Deserialize, Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DtAuthority {
    ProviderDerived,
    Normative,
}

#[derive(Deserialize, Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DtbProvider {
    Qemu,
    Firmware,
}

#[derive(Deserialize, Debug, Serialize)]
#[serde(deny_unknown_fields)]
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
        if let Some(qemu) = &config.qemu {
            if qemu.cpu.trim().is_empty() {
                anyhow::bail!("QEMU CPU model must not be empty");
            }
            validate_qemu(qemu)?;
        }
        match (&config.dtb, &config.qemu) {
            (Some(dtb), qemu) => dtb.validate(&config.build.arch, qemu.is_some())?,
            (None, Some(_)) => {
                anyhow::bail!("QEMU Platform must declare a provider=qemu DT contract")
            },
            (None, None) => {},
        }
        Ok(config)
    }
    pub fn gen_platform_defs(&self, root: &Root) -> String {
        let rootfs_source_path = root
            .source
            .path()
            .map(|path| format!("Some({path:?})"))
            .unwrap_or_else(|| "None".to_string());

        let earlycon_reg = self
            .constants
            .earlycon_reg
            .map(|addr| {
                format!(
                    "/// Physical address of the early console register\npub const EARLYCON_REG: PhysAddr = PhysAddr::new({addr:#x});\n"
                )
            })
            .unwrap_or_default();

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
{}/// Inclusive upper bound of firmware-visible physical CPU IDs
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
            earlycon_reg,
            self.constants.max_phys_cpu_id,
            self.constants.frame_section_shift_mb,
            root.fstype,
            root.source.kind(),
            rootfs_source_path,
        )
    }
}

fn validate_qemu(qemu: &Qemu) -> anyhow::Result<()> {
    for value in [&qemu.machine, &qemu.cpu, &qemu.smp, &qemu.memory]
        .into_iter()
        .chain(qemu.bios.iter())
        .chain(qemu.args.iter())
    {
        placeholder_names(value)?;
    }
    validate_qemu_bindings(&qemu.bind)
}

fn validate_qemu_bindings(bindings: &[QemuBind]) -> anyhow::Result<()> {
    let mut names = HashSet::new();
    for binding in bindings {
        validate_slug("QEMU bind", &binding.name)?;
        if !names.insert(binding.name.as_str()) {
            anyhow::bail!("duplicate QEMU bind `{}`", binding.name);
        }
        let mut placeholders = HashSet::new();
        for token in &binding.template {
            placeholders.extend(placeholder_names(token)?);
        }
        if placeholders != HashSet::from([binding.name.clone()]) {
            anyhow::bail!(
                "QEMU bind `{}` template must reference only `{{{{{}}}}}`",
                binding.name,
                binding.name
            );
        }
    }
    Ok(())
}

pub fn resolve_qemu_provider(
    qemu: &Qemu,
    values: &HashMap<String, String>,
    include_args: bool,
) -> anyhow::Result<(Qemu, HashSet<String>)> {
    let mut resolved = qemu.clone();
    let mut consumed = HashSet::new();
    resolved.machine = expand_placeholders(&qemu.machine, values, &mut consumed)?;
    resolved.cpu = expand_placeholders(&qemu.cpu, values, &mut consumed)?;
    resolved.smp = expand_placeholders(&qemu.smp, values, &mut consumed)?;
    resolved.memory = expand_placeholders(&qemu.memory, values, &mut consumed)?;
    resolved.bios = qemu
        .bios
        .as_deref()
        .map(|value| expand_placeholders(value, values, &mut consumed))
        .transpose()?;
    if include_args {
        resolved.args = qemu
            .args
            .iter()
            .map(|value| expand_placeholders(value, values, &mut consumed))
            .collect::<anyhow::Result<_>>()?;
    }
    Ok((resolved, consumed))
}

pub fn expand_placeholders(
    input: &str,
    values: &HashMap<String, String>,
    consumed: &mut HashSet<String>,
) -> anyhow::Result<String> {
    let mut output = String::new();
    let mut remainder = input;
    while let Some(start) = remainder.find("{{") {
        let literal = &remainder[..start];
        if literal.contains("}}") {
            anyhow::bail!("unsupported placeholder in `{input}`");
        }
        output.push_str(literal);
        let after_start = &remainder[start + 2..];
        let end = after_start
            .find("}}")
            .ok_or_else(|| anyhow::anyhow!("unterminated placeholder in `{input}`"))?;
        let name = &after_start[..end];
        if name.contains("{{") {
            anyhow::bail!("unsupported placeholder in `{input}`");
        }
        validate_slug("placeholder", name)?;
        let value = values
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("missing bind `{name}`"))?;
        output.push_str(value);
        consumed.insert(name.to_owned());
        remainder = &after_start[end + 2..];
    }
    if remainder.contains("}}") {
        anyhow::bail!("unsupported placeholder in `{input}`");
    }
    output.push_str(remainder);
    Ok(output)
}

pub fn placeholder_names(input: &str) -> anyhow::Result<HashSet<String>> {
    let mut names = HashSet::new();
    let mut remainder = input;
    while let Some(start) = remainder.find("{{") {
        if remainder[..start].contains("}}") {
            anyhow::bail!("unsupported placeholder in `{input}`");
        }
        let after_start = &remainder[start + 2..];
        let end = after_start
            .find("}}")
            .ok_or_else(|| anyhow::anyhow!("unterminated placeholder in `{input}`"))?;
        let name = &after_start[..end];
        if name.contains("{{") {
            anyhow::bail!("unsupported placeholder in `{input}`");
        }
        validate_slug("placeholder", name)?;
        names.insert(name.to_owned());
        remainder = &after_start[end + 2..];
    }
    if remainder.contains("}}") {
        anyhow::bail!("unsupported placeholder in `{input}`");
    }
    Ok(names)
}

impl Dtb {
    fn validate(&self, arch: &Arch, has_qemu: bool) -> anyhow::Result<()> {
        if let Some(source) = &self.source {
            let path = Path::new(source);
            if source.is_empty()
                || path.components().any(|component| {
                    matches!(
                        component,
                        Component::ParentDir | Component::RootDir | Component::Prefix(_)
                    )
                })
            {
                anyhow::bail!("DT source must be a workspace-relative path");
            }
        }

        match (arch, self.delivery) {
            (Arch::RiscV64, DtbDelivery::Firmware) | (Arch::LoongArch64, DtbDelivery::Embedded) => {
            },
            (Arch::RiscV64, DtbDelivery::Embedded) => {
                anyhow::bail!("riscv64 Platform requires firmware DT delivery")
            },
            (Arch::LoongArch64, DtbDelivery::Firmware) => {
                anyhow::bail!("loongarch64 Platform requires embedded DT delivery")
            },
        }

        match (
            has_qemu,
            self.delivery,
            self.authority,
            self.provider,
            self.source.is_some(),
        ) {
            (
                true,
                DtbDelivery::Firmware | DtbDelivery::Embedded,
                DtAuthority::ProviderDerived,
                Some(DtbProvider::Qemu),
                false,
            )
            | (
                false,
                DtbDelivery::Firmware,
                DtAuthority::ProviderDerived,
                Some(DtbProvider::Firmware),
                true,
            )
            | (false, DtbDelivery::Embedded, DtAuthority::Normative, None, true) => Ok(()),
            _ => anyhow::bail!(
                "invalid DT contract: QEMU Platforms require provider-derived provider=qemu without source; physical firmware baselines require provider-derived provider=firmware with source; physical embedded Platforms require normative source"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qemu_cpu_is_required_and_nonempty() {
        let valid = example_platform_text();
        assert!(Config::from_str(&valid.replace("cpu = \"rv64\"\n", "")).is_err());
        assert!(Config::from_str(&valid.replace("cpu = \"rv64\"", "cpu = \"\"")).is_err());
    }

    #[test]
    fn rejects_incoherent_dtb_contracts() {
        let valid = example_platform_text();

        for invalid in [
            valid.replace("provider = \"qemu\"\n", ""),
            valid.replace(
                "authority = \"provider-derived\"",
                "authority = \"normative\"",
            ),
            valid.replace("provider = \"qemu\"", "provider = \"other\""),
            valid.replace("[dtb]\n", "[dtb]\nsource = \"unexpected.dts\"\n"),
            valid.replace("delivery = \"firmware\"", "delivery = \"embedded\""),
        ] {
            assert!(Config::from_str(&invalid).is_err(), "{invalid}");
        }

        let mut embedded = example_platform();
        embedded.build.arch = Arch::LoongArch64;
        embedded.dtb.as_mut().unwrap().delivery = DtbDelivery::Embedded;
        let embedded = toml::to_string(&embedded).unwrap();
        assert!(
            Config::from_str(
                &embedded.replace("delivery = \"embedded\"", "delivery = \"firmware\"")
            )
            .is_err()
        );

        let physical = physical_platform(
            Arch::RiscV64,
            DtbDelivery::Firmware,
            DtAuthority::ProviderDerived,
            Some(DtbProvider::Firmware),
        );
        for invalid in [
            physical.replace("source = \"conf/platforms/example.dts\"\n", ""),
            physical.replace("source = \"conf/platforms/example.dts\"", "source = \"\""),
            physical.replace(
                "source = \"conf/platforms/example.dts\"",
                "source = \"../example.dts\"",
            ),
            physical.replace("provider = \"firmware\"", "provider = \"qemu\""),
        ] {
            assert!(Config::from_str(&invalid).is_err(), "{invalid}");
        }

        let normative = physical_platform(
            Arch::LoongArch64,
            DtbDelivery::Embedded,
            DtAuthority::Normative,
            None,
        );
        assert!(Config::from_str(&normative).is_ok());
        assert!(
            Config::from_str(&normative.replace("source = \"conf/platforms/example.dts\"\n", ""))
                .is_err()
        );
    }

    #[test]
    fn validates_qemu_bind_declarations() {
        let valid = example_platform_text()
            + r#"

[[qemu.bind]]
name = "disk-x0"
optional = true
template = ["-drive", "file={{disk-x0}},backup={{disk-x0}},format=raw"]
"#;
        let config = Config::from_str(&valid).unwrap();
        assert_eq!(config.qemu.unwrap().bind.len(), 2);

        for invalid in [
            valid.replace("name = \"disk-x0\"", "name = \"Disk_X0\""),
            valid.replace(
                "template = [\"-drive\", \"file={{disk-x0}},backup={{disk-x0}},format=raw\"]",
                "template = [\"-drive\", \"file=fixed,format=raw\"]",
            ),
            valid.replace("file={{disk-x0}}", "file={{path}}"),
            format!(
                "{valid}\n[[qemu.bind]]\nname = \"disk-x0\"\noptional = false\ntemplate = [\"file={{disk-x0}}\"]\n"
            ),
            format!("{valid}\nunknown = true\n"),
        ] {
            assert!(Config::from_str(&invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn provider_bind_expansion_is_named_and_single_pass() {
        let mut qemu = example_platform().qemu.unwrap();
        qemu.smp = "{{smp}}".to_string();
        qemu.memory = "{{memory}}".to_string();
        qemu.args = vec!["value={{runtime}}".to_string()];
        let values = HashMap::from([
            ("smp".to_string(), "8".to_string()),
            ("memory".to_string(), "{{not-recursive}}".to_string()),
            ("runtime".to_string(), "opaque, value".to_string()),
        ]);

        let (build, build_consumed) = resolve_qemu_provider(&qemu, &values, false).unwrap();
        assert_eq!(build.smp, "8");
        assert_eq!(build.memory, "{{not-recursive}}");
        assert_eq!(build.args, qemu.args);
        assert_eq!(
            build_consumed,
            HashSet::from(["smp".to_string(), "memory".to_string()])
        );

        let (runtime, runtime_consumed) = resolve_qemu_provider(&qemu, &values, true).unwrap();
        assert_eq!(runtime.args, ["value=opaque, value"]);
        assert_eq!(runtime_consumed.len(), 3);
    }

    fn example_platform_text() -> String {
        std::fs::read_to_string("../../conf/platforms/example.toml")
            .expect("failed to read example Platform")
    }

    fn example_platform() -> Config {
        Config::from_str(&example_platform_text()).unwrap()
    }

    fn physical_platform(
        arch: Arch,
        delivery: DtbDelivery,
        authority: DtAuthority,
        provider: Option<DtbProvider>,
    ) -> String {
        let mut platform = example_platform();
        platform.build.arch = arch;
        platform.qemu = None;
        let dtb = platform.dtb.as_mut().unwrap();
        dtb.source = Some("conf/platforms/example.dts".to_string());
        dtb.delivery = delivery;
        dtb.authority = authority;
        dtb.provider = provider;
        toml::to_string(&platform).unwrap()
    }
}
