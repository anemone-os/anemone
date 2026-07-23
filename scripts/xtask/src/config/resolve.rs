use std::{fs, path::Path};

use anyhow::Context;

use crate::workspace::{
    BUILD_PRESET_CONFIGS_PATH, DEF_KCONFIG_PATH, DEFAULT_SELECTION_PATH, LOCAL_SELECTION_PATH,
    PLATFORM_CONFIGS_PATH, SYSTEM_TARGET_CONFIGS_PATH,
};

use super::{
    KConfig, PlatformConfig,
    build_preset::{BuildPreset, CargoProfile},
    kconfig::KernelConfig,
    reference::{BuildPresetRef, KernelConfigRef, PlatformRef, SystemTargetRef},
    selection::{SelectionChoice, SelectionFile, SelectionRequest},
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
            SelectionChoice::Implicit => {
                let (selection, source) = self.load_implicit_selection()?;
                self.resolve_preset(selection.preset, source)
            },
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

    pub fn implicit_preset(&self) -> anyhow::Result<(BuildPresetRef, SelectionSource)> {
        let (selection, source) = self.load_implicit_selection()?;
        self.load_preset(&selection.preset)?;
        Ok((selection.preset, source))
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

    fn load_implicit_selection(&self) -> anyhow::Result<(SelectionFile, SelectionSource)> {
        let local_path = self.workspace_root.join(LOCAL_SELECTION_PATH);
        match fs::symlink_metadata(&local_path) {
            Ok(_) => {
                // Presence is decided from the directory entry, not the link
                // target. A dangling or unreadable local selection is invalid
                // state and must not silently select the tracked default.
                let content = fs::read_to_string(&local_path).with_context(|| {
                    format!("failed to read local selection at {}", local_path.display())
                })?;
                Ok((
                    SelectionFile::from_str(&content).with_context(|| {
                        format!(
                            "failed to parse local selection at {}",
                            local_path.display()
                        )
                    })?,
                    SelectionSource::LocalPreset,
                ))
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let default_path = self.workspace_root.join(DEFAULT_SELECTION_PATH);
                let content = fs::read_to_string(&default_path).with_context(|| {
                    format!(
                        "failed to read default selection at {}",
                        default_path.display()
                    )
                })?;
                Ok((
                    SelectionFile::from_str(&content).with_context(|| {
                        format!(
                            "failed to parse default selection at {}",
                            default_path.display()
                        )
                    })?,
                    SelectionSource::DefaultPreset,
                ))
            },
            Err(error) => Err(error).with_context(|| {
                format!(
                    "failed to inspect local selection at {}",
                    local_path.display()
                )
            }),
        }
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
    LocalPreset,
    DefaultPreset,
}

impl SelectionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExplicitPreset => "explicit-preset",
            Self::ExplicitTuple => "explicit-tuple",
            Self::LocalPreset => "local-preset",
            Self::DefaultPreset => "default-preset",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGETS: [(&str, &str); 6] = [
        ("example", "vda"),
        ("qemu-virt-rv64", "vda"),
        ("qemu-virt-rv64-pretest", "vda"),
        ("qemu-virt-la64", "vda"),
        ("qemu-virt-la64-pretest", "vda"),
        ("visionfive2-rv64", "mmcblk0"),
    ];

    const PRESETS: [(&str, &str, CargoProfile); 7] = [
        ("example", "example", CargoProfile::Release),
        (
            "qemu-virt-rv64-release",
            "qemu-virt-rv64",
            CargoProfile::Release,
        ),
        (
            "qemu-virt-rv64-pretest-release",
            "qemu-virt-rv64-pretest",
            CargoProfile::Release,
        ),
        (
            "qemu-virt-rv64-pretest-dev",
            "qemu-virt-rv64-pretest",
            CargoProfile::Dev,
        ),
        (
            "qemu-virt-la64-release",
            "qemu-virt-la64",
            CargoProfile::Release,
        ),
        (
            "qemu-virt-la64-pretest-release",
            "qemu-virt-la64-pretest",
            CargoProfile::Release,
        ),
        (
            "visionfive2-rv64-release",
            "visionfive2-rv64",
            CargoProfile::Release,
        ),
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
            assert_eq!(inputs.kernel_config_ref, kernel_config_ref);
            assert!(matches!(
                inputs.target.initial_program,
                super::super::system_target::InitialProgramSource::RootfsEntry
            ));
            assert!(!inputs.kernel_config.features.is_empty());
            assert_eq!(inputs.target.root.fstype, "ext4");
            match &inputs.target.root.source {
                super::super::system_target::RootSource::Block { path } => {
                    assert_eq!(path, expected_root_path);
                },
                super::super::system_target::RootSource::Pseudo => {
                    panic!("tracked target {target} unexpectedly uses pseudo root")
                },
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
    fn resolves_all_tracked_presets() {
        let loader = ConfigLoader::new(workspace_root());
        for (preset, target, profile) in PRESETS {
            let action = loader
                .resolve_selection(SelectionRequest::explicit_preset(
                    BuildPresetRef::new(preset).unwrap(),
                ))
                .unwrap_or_else(|error| panic!("failed to resolve preset {preset}: {error:#}"));
            assert_eq!(action.selection_source.as_str(), "explicit-preset");
            assert_eq!(action.system.target_ref.as_str(), target);
            assert_eq!(action.system.profile, profile);
            assert_eq!(
                action.system.kernel_config_ref.to_string(),
                "conf/.defconfig"
            );
        }
    }

    #[test]
    fn explicit_selection_does_not_read_invalid_local_state() {
        let workspace = TestWorkspace::new();
        fs::write(workspace.0.join(LOCAL_SELECTION_PATH), "not = [valid").unwrap();
        let loader = ConfigLoader::new(&workspace.0);

        let preset = loader
            .resolve_selection(SelectionRequest::explicit_preset(
                BuildPresetRef::new("test-release").unwrap(),
            ))
            .unwrap();
        assert_eq!(preset.selection_source.as_str(), "explicit-preset");

        let tuple = loader
            .resolve_selection(SelectionRequest::explicit_tuple(
                SystemTargetRef::new("qemu-virt-rv64-pretest").unwrap(),
                KernelConfigRef::new("kconfig").unwrap(),
                CargoProfile::Dev,
            ))
            .unwrap();
        assert_eq!(tuple.selection_source.as_str(), "explicit-tuple");
        assert_eq!(tuple.system.profile, CargoProfile::Dev);
    }

    #[test]
    fn implicit_selection_uses_local_or_absent_fallback_only() {
        let workspace = TestWorkspace::new();
        let loader = ConfigLoader::new(&workspace.0);

        let fallback = loader
            .resolve_selection(SelectionRequest::implicit())
            .unwrap();
        assert_eq!(fallback.selection_source.as_str(), "default-preset");
        assert_eq!(fallback.system.profile, CargoProfile::Release);

        fs::write(
            workspace.0.join(LOCAL_SELECTION_PATH),
            "preset = \"test-dev\"\n",
        )
        .unwrap();
        let local = loader
            .resolve_selection(SelectionRequest::implicit())
            .unwrap();
        assert_eq!(local.selection_source.as_str(), "local-preset");
        assert_eq!(local.system.profile, CargoProfile::Dev);

        fs::write(workspace.0.join(LOCAL_SELECTION_PATH), "invalid = true\n").unwrap();
        assert!(
            loader
                .resolve_selection(SelectionRequest::implicit())
                .is_err()
        );
        fs::write(
            workspace.0.join(LOCAL_SELECTION_PATH),
            "preset = \"missing-preset\"\n",
        )
        .unwrap();
        assert!(
            loader
                .resolve_selection(SelectionRequest::implicit())
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn dangling_local_selection_does_not_fall_back_to_default() {
        use std::os::unix::fs::symlink;

        let workspace = TestWorkspace::new();
        symlink(
            workspace.0.join("missing-selection-target"),
            workspace.0.join(LOCAL_SELECTION_PATH),
        )
        .unwrap();

        assert!(
            ConfigLoader::new(&workspace.0)
                .resolve_selection(SelectionRequest::implicit())
                .is_err()
        );
    }

    #[test]
    fn preset_rejects_missing_kernel_config() {
        let workspace = TestWorkspace::new();
        fs::write(
            workspace.0.join("conf/build-presets/missing-kconfig.toml"),
            "target = \"qemu-virt-rv64-pretest\"\nkernel-config = \"missing\"\nprofile = \"release\"\n",
        )
        .unwrap();
        let loader = ConfigLoader::new(&workspace.0);
        assert!(
            loader
                .resolve_selection(SelectionRequest::explicit_preset(
                    BuildPresetRef::new("missing-kconfig").unwrap(),
                ))
                .is_err()
        );
    }

    #[test]
    fn resolved_selection_owns_all_snapshot_inputs() {
        let workspace = TestWorkspace::new();
        let loader = ConfigLoader::new(&workspace.0);
        let action = loader
            .resolve_selection(SelectionRequest::implicit())
            .unwrap();
        let before = action.system.kernel_config.parameters.gen_kconfig_defs();

        fs::write(workspace.0.join(DEFAULT_SELECTION_PATH), "invalid = true\n").unwrap();
        fs::write(
            workspace.0.join(LOCAL_SELECTION_PATH),
            "preset = \"missing\"\n",
        )
        .unwrap();
        fs::write(
            workspace.0.join("conf/build-presets/test-release.toml"),
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
            workspace
                .0
                .join("conf/system-targets/qemu-virt-rv64-pretest.toml"),
            "fstype = \"ext4\"",
            "fstype = \"ramfs\"",
        );
        replace_file_text(
            workspace
                .0
                .join("conf/platforms/qemu-virt-rv64-pretest.toml"),
            "memory = \"1G\"",
            "memory = \"2G\"",
        );

        assert_eq!(action.system.target.root.fstype, "ext4");
        assert_eq!(action.system.platform.qemu.as_ref().unwrap().memory, "1G");
        assert_eq!(action.system.profile, CargoProfile::Release);
        assert!(before.contains("pub const MAX_LOGICAL_CPUS: usize = 16;"));
        assert!(before.contains("pub const SYSTEM_HZ: u16 = 100;"));
        assert_eq!(
            before,
            action.system.kernel_config.parameters.gen_kconfig_defs()
        );
    }

    #[test]
    fn kernel_config_rejects_legacy_build_selection() {
        let content = fs::read_to_string(workspace_root().join("conf/.defconfig")).unwrap();
        let legacy = format!(
            "[build]\ntarget = \"qemu-virt-rv64-pretest\"\nprofile = \"release\"\ndisasm = false\n\n{content}"
        );
        assert!(KConfig::from_str(&legacy).is_err());
    }

    #[test]
    fn resolved_snapshot_does_not_borrow_later_loader_values() {
        let workspace = TestWorkspace::new();
        let loader = ConfigLoader::new(&workspace.0);
        let action = loader
            .resolve_selection(SelectionRequest::explicit_preset(
                BuildPresetRef::new("test-release").unwrap(),
            ))
            .unwrap();
        let before = action.system.kernel_config.parameters.gen_kconfig_defs();

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
            workspace
                .0
                .join("conf/system-targets/qemu-virt-rv64-pretest.toml"),
            "fstype = \"ext4\"",
            "fstype = \"ramfs\"",
        );
        replace_file_text(
            workspace
                .0
                .join("conf/platforms/qemu-virt-rv64-pretest.toml"),
            "memory = \"1G\"",
            "memory = \"2G\"",
        );

        assert_eq!(action.system.target.root.fstype, "ext4");
        assert_eq!(action.system.platform.qemu.as_ref().unwrap().memory, "1G");
        assert!(before.contains("pub const MAX_LOGICAL_CPUS: usize = 16;"));
        assert!(before.contains("pub const SYSTEM_HZ: u16 = 100;"));
        assert_eq!(
            before,
            action.system.kernel_config.parameters.gen_kconfig_defs()
        );
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

            let default_content =
                fs::read_to_string(workspace_root().join("conf/.defconfig")).unwrap();
            fs::write(root.join("conf/.defconfig"), &default_content).unwrap();
            let selected_content = default_content
                .lines()
                .filter(|line| {
                    !line.trim_start().starts_with("max_logical_cpus")
                        && !line.trim_start().starts_with("ns16550a_default_baud")
                })
                .collect::<Vec<_>>()
                .join("\n");
            fs::write(root.join("kconfig"), selected_content).unwrap();

            for relative in [
                "conf/system-targets/qemu-virt-rv64-pretest.toml",
                "conf/platforms/qemu-virt-rv64-pretest.toml",
            ] {
                fs::copy(workspace_root().join(relative), root.join(relative)).unwrap();
            }
            for (name, profile) in [("test-release", "release"), ("test-dev", "dev")] {
                fs::write(
                    root.join(format!("conf/build-presets/{name}.toml")),
                    format!(
                        "target = \"qemu-virt-rv64-pretest\"\nkernel-config = \"kconfig\"\nprofile = \"{profile}\"\n"
                    ),
                )
                .unwrap();
            }
            fs::write(
                root.join(DEFAULT_SELECTION_PATH),
                "preset = \"test-release\"\n",
            )
            .unwrap();
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

    fn workspace_root() -> &'static Path {
        Path::new("../..")
    }
}
