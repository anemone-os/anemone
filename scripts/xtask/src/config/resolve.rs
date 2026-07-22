use std::{fs, path::Path};

use anyhow::Context;

use crate::workspace::{PLATFORM_CONFIGS_PATH, SYSTEM_TARGET_CONFIGS_PATH};

use super::{
    KConfig, PlatformConfig,
    kconfig::KernelConfig,
    reference::{KernelConfigRef, PlatformRef, SystemTargetRef},
    system_target::Config as SystemTargetConfig,
};

pub struct ConfigLoader<'a> {
    workspace_root: &'a Path,
}

impl<'a> ConfigLoader<'a> {
    pub fn new(workspace_root: &'a Path) -> Self {
        Self { workspace_root }
    }

    pub fn load_inputs(
        &self,
        target_ref: SystemTargetRef,
        kernel_config_ref: KernelConfigRef,
    ) -> anyhow::Result<LoadedSystemBuildInputs> {
        let target = self.load_target(&target_ref)?;
        let platform_ref = target.platform.clone();
        let platform = self.load_platform(&platform_ref)?;
        let kernel_config = self.load_kernel_config(&kernel_config_ref)?;
        Ok(LoadedSystemBuildInputs {
            target_ref,
            target,
            platform_ref,
            platform,
            kernel_config_ref,
            kernel_config,
        })
    }

    pub fn load_target(&self, target_ref: &SystemTargetRef) -> anyhow::Result<SystemTargetConfig> {
        let path = self
            .workspace_root
            .join(SYSTEM_TARGET_CONFIGS_PATH)
            .join(format!("{target_ref}.toml"));
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read system target `{target_ref}` at {}", path.display()))?;
        SystemTargetConfig::from_str(&content)
            .with_context(|| format!("failed to parse system target `{target_ref}`"))
    }

    pub fn load_platform(&self, platform_ref: &PlatformRef) -> anyhow::Result<PlatformConfig> {
        let path = self
            .workspace_root
            .join(PLATFORM_CONFIGS_PATH)
            .join(format!("{platform_ref}.toml"));
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read platform `{platform_ref}` at {}", path.display()))?;
        let platform = PlatformConfig::from_str(&content)
            .with_context(|| format!("failed to parse platform `{platform_ref}`"))?;
        ensure_platform_identity(platform_ref, &platform)?;
        Ok(platform)
    }

    pub fn load_kernel_config(
        &self,
        reference: &KernelConfigRef,
    ) -> anyhow::Result<KernelConfig> {
        let workspace_root = self
            .workspace_root
            .canonicalize()
            .context("failed to canonicalize workspace root")?;
        let path = workspace_root.join(reference.as_path());
        let canonical_path = path
            .canonicalize()
            .with_context(|| format!("failed to resolve kernel config `{reference}`"))?;
        if !canonical_path.starts_with(&workspace_root) {
            anyhow::bail!("kernel config `{reference}` escapes the workspace");
        }
        let metadata = fs::metadata(&canonical_path)
            .with_context(|| format!("failed to inspect kernel config `{reference}`"))?;
        if !metadata.is_file() {
            anyhow::bail!("kernel config `{reference}` is not a regular file");
        }
        let content = fs::read_to_string(&canonical_path)
            .with_context(|| format!("failed to read kernel config `{reference}`"))?;
        KConfig::from_str(&content)
            .with_context(|| format!("failed to parse kernel config `{reference}`"))
            .map(KConfig::into_kernel_config)
    }
}

pub struct LoadedSystemBuildInputs {
    pub target_ref: SystemTargetRef,
    pub target: SystemTargetConfig,
    pub platform_ref: PlatformRef,
    pub platform: PlatformConfig,
    pub kernel_config_ref: KernelConfigRef,
    pub kernel_config: KernelConfig,
}

fn ensure_platform_identity(
    platform_ref: &PlatformRef,
    platform: &PlatformConfig,
) -> anyhow::Result<()> {
    if platform.build.name != platform_ref.as_str() {
        anyhow::bail!(
            "platform filename identity `{platform_ref}` does not match legacy build.name `{}`",
            platform.build.name
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGETS: [(&str, &str); 5] = [
        ("qemu-virt-rv64", "vda"),
        ("qemu-virt-rv64-pretest", "vda"),
        ("qemu-virt-la64", "vda"),
        ("qemu-virt-la64-pretest", "vda"),
        ("visionfive2-rv64", "mmcblk0"),
    ];

    #[test]
    fn loads_all_supported_system_targets() {
        let loader = ConfigLoader::new(workspace_root());
        let kernel_config_ref = KernelConfigRef::new("conf/.defconfig").unwrap();

        for (target, expected_root_path) in TARGETS {
            let inputs = loader
                .load_inputs(
                    SystemTargetRef::new(target).unwrap(),
                    kernel_config_ref.clone(),
                )
                .unwrap_or_else(|error| panic!("failed to load {target}: {error:#}"));
            assert_eq!(inputs.target_ref.as_str(), target);
            assert_eq!(inputs.platform_ref.as_str(), target);
            assert_eq!(inputs.target.platform.as_str(), target);
            assert_eq!(inputs.platform.build.name, target);
            assert_eq!(inputs.kernel_config_ref, kernel_config_ref);
            assert!(matches!(
                inputs.target.initial_program,
                super::super::system_target::InitialProgramSource::RootfsEntry
            ));
            assert!(!inputs.kernel_config.features.is_empty());
            let legacy_root = inputs.platform.rootfs.as_ref().unwrap();
            assert_eq!(inputs.target.root.fstype, legacy_root.fstype);
            match &inputs.target.root.source {
                super::super::system_target::RootSource::Block { path } => {
                    assert_eq!(path, expected_root_path);
                    assert_eq!(legacy_root.source.path.as_deref(), Some(path.as_str()));
                }
                super::super::system_target::RootSource::Pseudo => {
                    panic!("tracked target {target} unexpectedly uses pseudo root")
                }
            }
        }
    }

    #[test]
    fn rejects_missing_canonical_inputs() {
        let loader = ConfigLoader::new(workspace_root());
        assert!(
            loader
                .load_target(&SystemTargetRef::new("missing-target").unwrap())
                .is_err()
        );
        assert!(
            loader
                .load_platform(&PlatformRef::new("missing-platform").unwrap())
                .is_err()
        );
        assert!(
            loader
                .load_kernel_config(&KernelConfigRef::new("conf/missing-kconfig").unwrap())
                .is_err()
        );
        assert!(
            loader
                .load_kernel_config(&KernelConfigRef::new("conf").unwrap())
                .is_err()
        );
    }

    #[test]
    fn rejects_platform_filename_name_mismatch() {
        let platform = PlatformConfig::from_str(
            r#"
[build]
name = "other-platform"
abbrs = []
arch = "riscv64"
exec_env = "sbi"

[constants]
phys_ram_start = 0x80000000
max_phys_ram_size = 0x80000000
kernel_la_base = 0x80200000
kernel_va_base = 0xffffffff80200000
max_phys_cpu_id = 0
frame_section_shift_mb = 7
"#,
        )
        .unwrap();
        assert!(
            ensure_platform_identity(&PlatformRef::new("expected-platform").unwrap(), &platform)
                .is_err()
        );
    }

    #[test]
    fn kernel_config_value_excludes_legacy_build_selection() {
        let content = fs::read_to_string(workspace_root().join("conf/.defconfig")).unwrap();
        let changed_selection = content
            .replace(
                "platform = \"qemu-virt-rv64-pretest\"",
                "platform = \"visionfive2-rv64\"",
            )
            .replace("profile = \"release\"", "profile = \"dev\"")
            .replace("disasm = false", "disasm = true");

        let original = KConfig::from_str(&content).unwrap().into_kernel_config();
        let changed = KConfig::from_str(&changed_selection)
            .unwrap()
            .into_kernel_config();
        assert_eq!(original, changed);
    }

    fn workspace_root() -> &'static Path {
        Path::new("../..")
    }
}
