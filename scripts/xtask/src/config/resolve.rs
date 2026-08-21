use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

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
            SelectionChoice::Preset(preset_ref) => self.resolve_preset(preset_ref),
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
        self.load_target_with_path(target_ref)
            .map(|(target, _)| target)
    }

    pub fn load_platform(&self, platform_ref: &PlatformRef) -> anyhow::Result<PlatformConfig> {
        self.load_platform_with_path(platform_ref)
            .map(|(platform, _)| platform)
    }

    pub fn load_kernel_config(&self, reference: &KernelConfigRef) -> anyhow::Result<KernelConfig> {
        self.load_resolved_kconfig(reference)
            .map(KConfig::into_kernel_config)
    }

    pub fn load_preset(&self, preset_ref: &BuildPresetRef) -> anyhow::Result<BuildPreset> {
        self.load_preset_with_path(preset_ref)
            .map(|(preset, _)| preset)
    }

    fn load_target_with_path(
        &self,
        target_ref: &SystemTargetRef,
    ) -> anyhow::Result<(SystemTargetConfig, PathBuf)> {
        let (content, path) = self.read_config_file(
            "system target",
            SYSTEM_TARGET_CONFIGS_PATH,
            target_ref.as_path(),
            target_ref.canonical_name(),
        )?;
        let target = SystemTargetConfig::from_str(&content).with_context(|| {
            format!(
                "failed to parse system target `{target_ref}` at {}",
                path.display()
            )
        })?;
        Ok((target, path))
    }

    fn load_platform_with_path(
        &self,
        platform_ref: &PlatformRef,
    ) -> anyhow::Result<(PlatformConfig, PathBuf)> {
        let (content, path) = self.read_config_file(
            "platform",
            PLATFORM_CONFIGS_PATH,
            platform_ref.as_path(),
            platform_ref.canonical_name(),
        )?;
        let platform = PlatformConfig::from_str(&content).with_context(|| {
            format!(
                "failed to parse platform `{platform_ref}` at {}",
                path.display()
            )
        })?;
        Ok((platform, path))
    }

    fn load_preset_with_path(
        &self,
        preset_ref: &BuildPresetRef,
    ) -> anyhow::Result<(BuildPreset, PathBuf)> {
        let (content, path) = self.read_config_file(
            "build preset",
            BUILD_PRESET_CONFIGS_PATH,
            preset_ref.as_path(),
            preset_ref.canonical_name(),
        )?;
        let preset = BuildPreset::from_str(&content).with_context(|| {
            format!(
                "failed to parse build preset `{preset_ref}` at {}",
                path.display()
            )
        })?;
        Ok((preset, path))
    }

    fn read_config_file(
        &self,
        kind: &str,
        canonical_dir: &str,
        fallback_path: &Path,
        canonical_name: Option<&str>,
    ) -> anyhow::Result<(String, PathBuf)> {
        let workspace_root = self
            .workspace_root
            .canonicalize()
            .context("failed to canonicalize workspace root")?;
        let canonical_path =
            canonical_name.map(|name| Path::new(canonical_dir).join(format!("{name}.toml")));
        let (selected_path, route) = if let Some(path) = &canonical_path {
            match fs::symlink_metadata(workspace_root.join(path)) {
                Ok(_) => (path.clone(), format!("canonical path {}", path.display())),
                Err(error) if error.kind() == ErrorKind::NotFound => (
                    fallback_path.to_owned(),
                    format!(
                        "canonical path {} was not found; workspace fallback {}",
                        path.display(),
                        fallback_path.display()
                    ),
                ),
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("failed to inspect canonical {kind} path {}", path.display())
                    });
                },
            }
        } else {
            (
                fallback_path.to_owned(),
                format!("workspace path {}", fallback_path.display()),
            )
        };

        let selected = workspace_root.join(&selected_path);
        let resolved = selected
            .canonicalize()
            .with_context(|| format!("failed to resolve {kind} ({route})"))?;
        if !resolved.starts_with(&workspace_root) {
            anyhow::bail!(
                "{kind} path {} escapes the workspace",
                selected_path.display()
            );
        }
        let metadata = fs::metadata(&resolved)
            .with_context(|| format!("failed to inspect {kind} at {}", selected_path.display()))?;
        if !metadata.is_file() {
            anyhow::bail!(
                "{kind} path {} is not a regular file",
                selected_path.display()
            );
        }
        let content = fs::read_to_string(&resolved)
            .with_context(|| format!("failed to read {kind} at {}", selected_path.display()))?;
        Ok((content, selected_path))
    }

    fn resolve_preset(&self, preset_ref: BuildPresetRef) -> anyhow::Result<ResolvedSelection> {
        let (preset, path) = self.load_preset_with_path(&preset_ref)?;
        self.resolve_references(
            preset.target,
            preset.kernel_config,
            preset.profile,
            SelectionSource::ExplicitPreset { path },
        )
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
        let (target, target_path) = self.load_target_with_path(&target_ref)?;
        validate_nemophila_selection(&target, &kernel_config)?;
        let platform_ref = target.platform.clone();
        let (platform, platform_path) = self.load_platform_with_path(&platform_ref)?;
        Ok(ResolvedSystemBuild {
            target_ref,
            target_path,
            target,
            platform_ref,
            platform_path,
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

fn validate_nemophila_selection(
    target: &SystemTargetConfig,
    kernel_config: &KernelConfig,
) -> anyhow::Result<()> {
    let enabled = kernel_config
        .features
        .get("nemophila")
        .copied()
        .unwrap_or(false);
    if !enabled && !target.nemophila.is_empty() {
        anyhow::bail!(
            "SystemTarget selects embedded Nemophila modules {:?}, but KernelConfig feature `nemophila` is disabled",
            target.nemophila
        );
    }
    Ok(())
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
    /// Resolution provenance for diagnostics; behavior uses the parsed target
    /// snapshot.
    pub target_path: PathBuf,
    pub target: SystemTargetConfig,
    pub platform_ref: PlatformRef,
    /// Resolution provenance for diagnostics; behavior uses the parsed Platform
    /// snapshot.
    pub platform_path: PathBuf,
    pub platform: PlatformConfig,
    pub kernel_config_ref: KernelConfigRef,
    pub kernel_config: KernelConfig,
    pub profile: CargoProfile,
}

/// Selection provenance is diagnostic-only after the resolver owns the parsed
/// snapshot.
#[derive(Clone)]
pub enum SelectionSource {
    ExplicitPreset { path: PathBuf },
    ExplicitTuple,
}

impl SelectionSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ExplicitPreset { .. } => "explicit-preset",
            Self::ExplicitTuple => "explicit-tuple",
        }
    }

    pub fn config_path(&self) -> Option<&Path> {
        match self {
            Self::ExplicitPreset { path } => Some(path),
            Self::ExplicitTuple => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_test_preset_and_tuple() {
        let workspace = TestWorkspace::new();
        let loader = ConfigLoader::new(&workspace.root);

        let preset = loader
            .resolve_selection(SelectionRequest::explicit_preset(
                BuildPresetRef::new("example").unwrap(),
            ))
            .unwrap();
        assert_eq!(preset.selection_source.as_str(), "explicit-preset");
        assert_eq!(preset.system.target_ref.as_str(), "example");
        assert_eq!(
            preset.selection_source.config_path(),
            Some(Path::new("conf/build-presets/example.toml"))
        );
        assert_eq!(
            preset.system.target_path,
            Path::new("conf/system-targets/example.toml")
        );
        assert_eq!(preset.system.platform_ref.as_str(), "example");
        assert_eq!(
            preset.system.platform_path,
            Path::new("conf/platforms/example.toml")
        );
        assert_eq!(preset.system.profile, CargoProfile::Release);

        let tuple = loader
            .resolve_selection(SelectionRequest::explicit_tuple(
                SystemTargetRef::new("example").unwrap(),
                KernelConfigRef::new("kconfig").unwrap(),
                CargoProfile::Release,
            ))
            .unwrap();
        assert_eq!(tuple.selection_source.as_str(), "explicit-tuple");
        assert_eq!(tuple.selection_source.config_path(), None);
        assert_eq!(tuple.system.target_ref.as_str(), "example");
        assert_eq!(tuple.system.platform_ref.as_str(), "example");
        assert_eq!(tuple.system.profile, CargoProfile::Release);
    }

    #[test]
    fn resolves_workspace_fallback_config_graph() {
        let workspace = TestWorkspace::new();
        fs::create_dir_all(workspace.root.join("local")).unwrap();
        fs::copy(
            workspace.root.join("conf/platforms/example.toml"),
            workspace.root.join("local/private-platform"),
        )
        .unwrap();
        let target = fs::read_to_string(workspace.root.join("conf/system-targets/example.toml"))
            .unwrap()
            .replace(
                "platform = \"example\"",
                "platform = \"local/private-platform\"",
            );
        fs::write(workspace.root.join("local/private-target"), target).unwrap();
        let preset = fs::read_to_string(workspace.root.join("conf/build-presets/example.toml"))
            .unwrap()
            .replace("target = \"example\"", "target = \"local/private-target\"");
        fs::write(workspace.root.join("private-preset"), preset).unwrap();

        let action = ConfigLoader::new(&workspace.root)
            .resolve_selection(SelectionRequest::explicit_preset(
                BuildPresetRef::new("private-preset").unwrap(),
            ))
            .unwrap();

        assert_eq!(
            action.selection_source.config_path(),
            Some(Path::new("private-preset"))
        );
        assert_eq!(action.system.target_path, Path::new("local/private-target"));
        assert_eq!(
            action.system.platform_path,
            Path::new("local/private-platform")
        );
    }

    #[test]
    fn canonical_config_wins_unless_workspace_path_is_explicit() {
        let workspace = TestWorkspace::new();
        let root_preset =
            fs::read_to_string(workspace.root.join("conf/build-presets/example.toml"))
                .unwrap()
                .replace("target = \"example\"", "target = \"root-target\"");
        fs::write(workspace.root.join("example"), root_preset).unwrap();
        let loader = ConfigLoader::new(&workspace.root);

        let (canonical, canonical_path) = loader
            .load_preset_with_path(&BuildPresetRef::new("example").unwrap())
            .unwrap();
        assert_eq!(canonical.target.as_str(), "example");
        assert_eq!(canonical_path, Path::new("conf/build-presets/example.toml"));

        let (explicit, explicit_path) = loader
            .load_preset_with_path(&BuildPresetRef::new("./example").unwrap())
            .unwrap();
        assert_eq!(explicit.target.as_str(), "root-target");
        assert_eq!(explicit_path, Path::new("example"));
    }

    #[test]
    fn existing_invalid_canonical_config_does_not_fallback() {
        let workspace = TestWorkspace::new();
        fs::copy(
            workspace.root.join("conf/build-presets/example.toml"),
            workspace.root.join("shadowed"),
        )
        .unwrap();
        fs::write(
            workspace.root.join("conf/build-presets/shadowed.toml"),
            "invalid = true\n",
        )
        .unwrap();

        let error = ConfigLoader::new(&workspace.root)
            .load_preset(&BuildPresetRef::new("shadowed").unwrap())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("conf/build-presets/shadowed.toml"),
            "{error}"
        );
    }

    #[test]
    fn config_paths_reject_workspace_escape_and_report_both_missing_candidates() {
        use std::os::unix::fs::symlink;

        let workspace = TestWorkspace::new();
        symlink("/etc/hosts", workspace.root.join("escaped-preset")).unwrap();
        let loader = ConfigLoader::new(&workspace.root);

        let escape = loader
            .load_preset(&BuildPresetRef::new("./escaped-preset").unwrap())
            .unwrap_err()
            .to_string();
        assert!(escape.contains("escapes the workspace"), "{escape}");

        let missing = loader
            .load_preset(&BuildPresetRef::new("missing-preset").unwrap())
            .unwrap_err()
            .to_string();
        assert!(
            missing.contains("conf/build-presets/missing-preset.toml"),
            "{missing}"
        );
        assert!(
            missing.contains("workspace fallback missing-preset"),
            "{missing}"
        );
    }

    #[test]
    fn rejects_missing_canonical_inputs() {
        let workspace = TestWorkspace::new();
        let loader = ConfigLoader::new(&workspace.root);
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
        fs::write(
            workspace.root.join("conf/build-presets/example.toml"),
            super::super::build_preset::TEST_BUILD_PRESET
                .replace("kernel-config = \"kconfig\"", "kernel-config = \"missing\""),
        )
        .unwrap();
        let loader = ConfigLoader::new(&workspace.root);
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
        let loader = ConfigLoader::new(&workspace.root);
        let default_max_logical_cpus = loader
            .load_kernel_config(&KernelConfigRef::new(DEF_KCONFIG_PATH).unwrap())
            .unwrap()
            .parameters
            .max_logical_cpus;
        let action = loader
            .resolve_selection(SelectionRequest::explicit_preset(
                BuildPresetRef::new("example").unwrap(),
            ))
            .unwrap();

        for relative in [
            "kconfig",
            "conf/kconfs/default.toml",
            "conf/system-targets/example.toml",
            "conf/platforms/example.toml",
            "conf/build-presets/example.toml",
        ] {
            fs::write(workspace.root.join(relative), "invalid = true\n").unwrap();
        }

        assert_eq!(action.system.target.root.fstype, "ext4");
        assert_eq!(action.system.platform.qemu.as_ref().unwrap().memory, "1G");
        assert_eq!(action.system.profile, CargoProfile::Release);
        assert_eq!(
            action.system.kernel_config.parameters.max_logical_cpus,
            default_max_logical_cpus
        );
        assert_eq!(action.system.kernel_config.parameters.system_hz, Some(777));
    }

    #[test]
    fn kernel_config_rejects_legacy_build_selection() {
        let workspace = TestWorkspace::new();
        let default_kconfig =
            fs::read_to_string(workspace.root.join("conf/kconfs/default.toml")).unwrap();
        let legacy = format!(
            "[build]\ntarget = \"example\"\nprofile = \"release\"\ndisasm = false\n\n{default_kconfig}"
        );
        assert!(KConfig::from_str(&legacy).is_err());
    }

    #[test]
    fn embedded_nemophila_selection_requires_the_kernel_feature() {
        let workspace = TestWorkspace::new();
        let target_path = workspace.root.join("conf/system-targets/example.toml");
        let target = fs::read_to_string(&target_path).unwrap().replacen(
            "platform = \"example\"",
            "platform = \"example\"\nnemophila = [\"clone-observer\"]",
            1,
        );
        fs::write(target_path, target).unwrap();
        let loader = ConfigLoader::new(&workspace.root);

        let error = loader
            .resolve_selection(SelectionRequest::explicit_tuple(
                SystemTargetRef::new("example").unwrap(),
                KernelConfigRef::new("kconfig").unwrap(),
                CargoProfile::Release,
            ))
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("feature `nemophila` is disabled"), "{error}");

        let kconfig_path = workspace.root.join("kconfig");
        let mut kconfig = fs::read_to_string(&kconfig_path)
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        kconfig["features"]["nemophila"] = toml_edit::value(true);
        fs::write(kconfig_path, kconfig.to_string()).unwrap();
        let resolved = loader
            .resolve_selection(SelectionRequest::explicit_tuple(
                SystemTargetRef::new("example").unwrap(),
                KernelConfigRef::new("kconfig").unwrap(),
                CargoProfile::Release,
            ))
            .unwrap();
        assert_eq!(
            resolved.system.target.nemophila,
            vec!["clone-observer".to_string()]
        );
    }

    struct TestWorkspace {
        root: std::path::PathBuf,
    }

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
            fs::create_dir_all(root.join("conf/kconfs")).unwrap();

            fs::write(
                root.join("conf/system-targets/example.toml"),
                super::super::system_target::TEST_SYSTEM_TARGET,
            )
            .unwrap();
            fs::write(
                root.join("conf/platforms/example.toml"),
                super::super::platform::TEST_QEMU_PLATFORM,
            )
            .unwrap();
            fs::write(
                root.join("conf/build-presets/example.toml"),
                super::super::build_preset::TEST_BUILD_PRESET,
            )
            .unwrap();

            // The canonical default is intentionally the only repository configuration
            // used by resolver tests: its complete parameter inventory is the default
            // materialization contract and must not be mirrored by a test fixture.
            let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let default_content = fs::read_to_string(repository.join(DEF_KCONFIG_PATH)).unwrap();
            fs::write(root.join(DEF_KCONFIG_PATH), &default_content).unwrap();

            let mut selected = default_content
                .parse::<toml_edit::DocumentMut>()
                .expect("default KernelConfig must be valid TOML");
            selected["parameters"]["system_hz"] = toml_edit::value(777);
            selected["parameters"]
                .as_table_mut()
                .unwrap()
                .remove("max_logical_cpus");
            fs::write(root.join("kconfig"), selected.to_string()).unwrap();

            Self { root }
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
