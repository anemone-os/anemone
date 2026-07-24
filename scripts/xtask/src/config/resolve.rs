use std::{fs, path::Path};

use anyhow::Context;

use crate::workspace::{
    BUILD_PRESET_CONFIGS_PATH, DEF_KCONFIG_PATH, PLATFORM_CONFIGS_PATH, SYSTEM_TARGET_CONFIGS_PATH,
};

use super::{
    KConfig, PlatformConfig,
    build_preset::{BuildPreset, CargoProfile},
    kconfig::KernelConfig,
    reference::{BuildPresetRef, KernelConfigRef, PlatformRef, SystemTargetRef},
    selection::{SelectionChoice, SelectionRequest},
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

    pub fn resolve_selection(
        &self,
        request: SelectionRequest,
    ) -> anyhow::Result<ResolvedSelection> {
        match request.classify()? {
            SelectionChoice::Preset(preset_ref) => {
                self.resolve_preset(preset_ref, SelectionSource::ExplicitPreset)
            },
            SelectionChoice::Tuple {
                target,
                kernel_config,
                profile,
            } => self.resolve_references(
                target,
                kernel_config,
                profile,
                SelectionSource::ExplicitTuple,
            ),
        }
    }

    pub fn load_target(&self, target_ref: &SystemTargetRef) -> anyhow::Result<SystemTargetConfig> {
        let path = self
            .workspace_root
            .join(SYSTEM_TARGET_CONFIGS_PATH)
            .join(format!("{target_ref}.toml"));
        let content = fs::read_to_string(&path).with_context(|| {
            format!(
                "failed to read system target `{target_ref}` at {}",
                path.display()
            )
        })?;
        SystemTargetConfig::from_str(&content)
            .with_context(|| format!("failed to parse system target `{target_ref}`"))
    }

    pub fn load_platform(&self, platform_ref: &PlatformRef) -> anyhow::Result<PlatformConfig> {
        let path = self
            .workspace_root
            .join(PLATFORM_CONFIGS_PATH)
            .join(format!("{platform_ref}.toml"));
        let content = fs::read_to_string(&path).with_context(|| {
            format!(
                "failed to read platform `{platform_ref}` at {}",
                path.display()
            )
        })?;
        PlatformConfig::from_str(&content)
            .with_context(|| format!("failed to parse platform `{platform_ref}`"))
    }

    pub fn load_kernel_config(&self, reference: &KernelConfigRef) -> anyhow::Result<KernelConfig> {
        self.load_resolved_kconfig(reference)
            .map(KConfig::into_kernel_config)
    }

    pub fn load_preset(&self, preset_ref: &BuildPresetRef) -> anyhow::Result<BuildPreset> {
        let path = self
            .workspace_root
            .join(BUILD_PRESET_CONFIGS_PATH)
            .join(format!("{preset_ref}.toml"));
        let content = fs::read_to_string(&path).with_context(|| {
            format!(
                "failed to read build preset `{preset_ref}` at {}",
                path.display()
            )
        })?;
        BuildPreset::from_str(&content)
            .with_context(|| format!("failed to parse build preset `{preset_ref}`"))
    }

    fn resolve_preset(
        &self,
        preset_ref: BuildPresetRef,
        source: SelectionSource,
    ) -> anyhow::Result<ResolvedSelection> {
        let preset = self.load_preset(&preset_ref)?;
        self.resolve_references(preset.target, preset.kernel_config, preset.profile, source)
    }

    fn resolve_references(
        &self,
        target_ref: SystemTargetRef,
        kernel_config_ref: KernelConfigRef,
        profile: CargoProfile,
        source: SelectionSource,
    ) -> anyhow::Result<ResolvedSelection> {
        let kernel_config = self.load_kernel_config(&kernel_config_ref)?;
        let system =
            self.resolve_owned_system(target_ref, kernel_config_ref, kernel_config, profile)?;
        Ok(ResolvedSelection {
            selection_source: source,
            system,
        })
    }

    fn resolve_owned_system(
        &self,
        target_ref: SystemTargetRef,
        kernel_config_ref: KernelConfigRef,
        kernel_config: KernelConfig,
        profile: CargoProfile,
    ) -> anyhow::Result<ResolvedSystemBuild> {
        let target = self.load_target(&target_ref)?;
        let platform_ref = target.platform.clone();
        let platform = self.load_platform(&platform_ref)?;
        Ok(ResolvedSystemBuild {
            target_ref,
            target,
            platform_ref,
            platform,
            kernel_config_ref,
            kernel_config,
            profile,
        })
    }

    fn load_resolved_kconfig(&self, reference: &KernelConfigRef) -> anyhow::Result<KConfig> {
        let mut config = self.load_kconfig(reference)?;
        if reference.as_path() == Path::new(DEF_KCONFIG_PATH) {
            config.parameters.materialize_defaults(None)?;
        } else {
            let default_ref = KernelConfigRef::new(DEF_KCONFIG_PATH)?;
            let defaults = self.load_kconfig(&default_ref)?;
            config
                .parameters
                .materialize_defaults(Some(&defaults.parameters))?;
        }
        Ok(config)
    }

    fn load_kconfig(&self, reference: &KernelConfigRef) -> anyhow::Result<KConfig> {
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

pub struct ResolvedSelection {
    pub selection_source: SelectionSource,
    pub system: ResolvedSystemBuild,
}

pub struct ResolvedSystemBuild {
    pub target_ref: SystemTargetRef,
    pub target: SystemTargetConfig,
    pub platform_ref: PlatformRef,
    pub platform: PlatformConfig,
    pub kernel_config_ref: KernelConfigRef,
    pub kernel_config: KernelConfig,
    pub profile: CargoProfile,
}

#[derive(Clone, Copy)]
pub enum SelectionSource {
    ExplicitPreset,
    ExplicitTuple,
}

impl SelectionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExplicitPreset => "explicit-preset",
            Self::ExplicitTuple => "explicit-tuple",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_example_preset_and_tuple() {
        let workspace = TestWorkspace::new();
        let loader = ConfigLoader::new(&workspace.0);

        let preset = loader
            .resolve_selection(SelectionRequest::explicit_preset(
                BuildPresetRef::new("example").unwrap(),
            ))
            .unwrap();
        assert_eq!(preset.selection_source.as_str(), "explicit-preset");
        assert_eq!(preset.system.target_ref.as_str(), "example");
        assert_eq!(preset.system.platform_ref.as_str(), "example");
        assert_eq!(preset.system.profile, CargoProfile::Release);

        let tuple = loader
            .resolve_selection(SelectionRequest::explicit_tuple(
                SystemTargetRef::new("example").unwrap(),
                KernelConfigRef::new("kconfig").unwrap(),
                CargoProfile::Release,
            ))
            .unwrap();
        assert_eq!(tuple.selection_source.as_str(), "explicit-tuple");
        assert_eq!(tuple.system.target_ref.as_str(), "example");
        assert_eq!(tuple.system.platform_ref.as_str(), "example");
        assert_eq!(tuple.system.profile, CargoProfile::Release);
    }

    #[test]
    fn rejects_missing_canonical_inputs() {
        let workspace = TestWorkspace::new();
        let loader = ConfigLoader::new(&workspace.0);
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
    fn preset_rejects_missing_kernel_config() {
        let workspace = TestWorkspace::new();
        replace_file_text(
            workspace.0.join("conf/build-presets/example.toml"),
            "kernel-config = \"kconfig\"",
            "kernel-config = \"missing\"",
        );
        let loader = ConfigLoader::new(&workspace.0);
        assert!(
            loader
                .resolve_selection(SelectionRequest::explicit_preset(
                    BuildPresetRef::new("example").unwrap(),
                ))
                .is_err()
        );
    }

    #[test]
    fn resolved_selection_owns_all_snapshot_inputs() {
        let workspace = TestWorkspace::new();
        let loader = ConfigLoader::new(&workspace.0);
        let action = loader
            .resolve_selection(SelectionRequest::explicit_preset(
                BuildPresetRef::new("example").unwrap(),
            ))
            .unwrap();

        fs::write(
            workspace.0.join("conf/build-presets/example.toml"),
            "invalid = true\n",
        )
        .unwrap();
        replace_file_text(
            workspace.0.join("kconfig"),
            "system_hz = 100",
            "system_hz = 999",
        );
        replace_file_text(
            workspace.0.join("conf/.defconfig"),
            "max_logical_cpus = 16",
            "max_logical_cpus = 99",
        );
        replace_file_text(
            workspace.0.join("conf/system-targets/example.toml"),
            "fstype = \"ext4\"",
            "fstype = \"ramfs\"",
        );
        replace_file_text(
            workspace.0.join("conf/platforms/example.toml"),
            "memory = \"1G\"",
            "memory = \"2G\"",
        );

        assert_eq!(action.system.target.root.fstype, "ext4");
        assert_eq!(action.system.platform.qemu.as_ref().unwrap().memory, "1G");
        assert_eq!(action.system.profile, CargoProfile::Release);
        assert_eq!(
            action.system.kernel_config.parameters.max_logical_cpus,
            Some(16)
        );
        assert_eq!(action.system.kernel_config.parameters.system_hz, Some(100));
    }

    #[test]
    fn kernel_config_rejects_legacy_build_selection() {
        let workspace = TestWorkspace::new();
        let default_kconfig = fs::read_to_string(workspace.0.join("conf/.defconfig")).unwrap();
        let legacy = format!(
            "[build]\ntarget = \"example\"\nprofile = \"release\"\ndisasm = false\n\n{default_kconfig}"
        );
        assert!(KConfig::from_str(&legacy).is_err());
    }

    struct TestWorkspace(std::path::PathBuf);

    impl TestWorkspace {
        fn new() -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "anemone-xtask-resolve-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(root.join("conf/system-targets")).unwrap();
            fs::create_dir_all(root.join("conf/platforms")).unwrap();
            fs::create_dir_all(root.join("conf/build-presets")).unwrap();

            for relative in [
                "conf/.defconfig",
                "conf/system-targets/example.toml",
                "conf/platforms/example.toml",
                "conf/build-presets/example.toml",
            ] {
                fs::copy(Path::new("../..").join(relative), root.join(relative)).unwrap();
            }

            let default_content = fs::read_to_string(root.join("conf/.defconfig")).unwrap();
            let selected_content = default_content
                .lines()
                .filter(|line| {
                    !line.trim_start().starts_with("max_logical_cpus")
                        && !line.trim_start().starts_with("ns16550a_default_baud")
                })
                .collect::<Vec<_>>()
                .join("\n");
            fs::write(root.join("kconfig"), selected_content).unwrap();
            replace_file_text(
                root.join("conf/build-presets/example.toml"),
                "kernel-config = \"conf/.defconfig\"",
                "kernel-config = \"kconfig\"",
            );
            Self(root)
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn replace_file_text(path: impl AsRef<Path>, old: &str, new: &str) {
        let path = path.as_ref();
        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains(old));
        fs::write(path, content.replace(old, new)).unwrap();
    }
}
