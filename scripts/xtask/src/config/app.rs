//! Manifest for applications under anemone-apps.

use std::collections::HashSet;

use anyhow::Context;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use super::platform::{Arch, TargetTriple};

/// Maximum final Command argv length, including the program and CLI extras.
///
/// Command is trusted repository build code, but rejecting accidental argv
/// explosions here gives manifests a stable failure boundary before host exec.
pub const MAX_COMMAND_ARGUMENTS: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct App {
    pub name: String,
    pub targets: Vec<AppTarget>,
    pub build: Build,
    pub artifacts: Vec<Artifact>,
}

/// Target selected for one app build action.
///
/// Anemone targets reuse the Platform architecture owner. `Host` is app-local
/// and must not become a kernel, rootfs, or QEMU architecture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppTarget {
    Anemone(Arch),
    Host,
}

impl AppTarget {
    pub fn try_from_str(value: &str) -> anyhow::Result<Self> {
        if value == "host" {
            return Ok(Self::Host);
        }

        Arch::try_from_str(value)
            .map(Self::Anemone)
            .map_err(|_| anyhow::anyhow!("Unsupported app build target: {value}"))
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Anemone(arch) => arch.as_str(),
            Self::Host => "host",
        }
    }

    pub fn target_triple(&self) -> Option<TargetTriple> {
        match self {
            Self::Anemone(arch) => Some(arch.target_triple()),
            Self::Host => None,
        }
    }

    pub fn is_host(&self) -> bool {
        matches!(self, Self::Host)
    }
}

impl From<Arch> for AppTarget {
    fn from(arch: Arch) -> Self {
        Self::Anemone(arch)
    }
}

impl Serialize for AppTarget {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AppTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from_str(&value).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Build {
    pub workdir: String,

    #[serde(flatten)]
    pub driver: BuildDriver,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "driver")]
pub enum BuildDriver {
    #[serde(rename = "cargo")]
    Cargo(CargoBuild),
    #[serde(rename = "command")]
    Command(CommandBuild),
    #[serde(rename = "source")]
    Source(SourceBuild),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoBuild {
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandBuild {
    pub argv: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceBuild {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub path: String,
    #[serde(default)]
    pub targets: Option<Vec<AppTarget>>,
}

impl Artifact {
    pub fn supports(&self, target: &AppTarget) -> bool {
        self.targets
            .as_ref()
            .is_none_or(|targets| targets.contains(target))
    }
}

impl App {
    pub fn from_str(content: &str) -> anyhow::Result<Self> {
        let manifest: App = toml::from_str(content)
            .with_context(|| "Failed to parse app manifest from string content")?;
        anyhow::ensure!(
            !manifest.targets.is_empty(),
            "app target list must not be empty"
        );
        anyhow::ensure!(
            !manifest.artifacts.is_empty(),
            "app artifact list must not be empty"
        );
        let mut targets = HashSet::new();
        for target in &manifest.targets {
            anyhow::ensure!(
                targets.insert(target.as_str()),
                "duplicate app build target '{}'",
                target.as_str()
            );
        }
        for artifact in &manifest.artifacts {
            let Some(artifact_targets) = &artifact.targets else {
                continue;
            };
            anyhow::ensure!(
                !artifact_targets.is_empty(),
                "artifact '{}' target list must not be empty",
                artifact.path
            );
            let mut targets = HashSet::new();
            for target in artifact_targets {
                anyhow::ensure!(
                    targets.insert(target.as_str()),
                    "artifact '{}' has duplicate build target '{}'",
                    artifact.path,
                    target.as_str()
                );
                anyhow::ensure!(
                    manifest.targets.contains(target),
                    "artifact '{}' selects target '{}' not declared by app '{}'",
                    artifact.path,
                    target.as_str(),
                    manifest.name
                );
            }
        }
        for target in &manifest.targets {
            anyhow::ensure!(
                manifest
                    .artifacts
                    .iter()
                    .any(|artifact| artifact.supports(target)),
                "app '{}' target '{}' has no artifact",
                manifest.name,
                target.as_str()
            );
        }
        if let BuildDriver::Command(build) = &manifest.build.driver {
            anyhow::ensure!(
                !build.argv.is_empty(),
                "command driver argv must not be empty"
            );
            anyhow::ensure!(
                build.argv.len() <= MAX_COMMAND_ARGUMENTS,
                "command driver argv has {} arguments, exceeding the maximum of {}",
                build.argv.len(),
                MAX_COMMAND_ARGUMENTS
            );
        }
        if manifest.targets.iter().any(AppTarget::is_host) {
            anyhow::ensure!(
                !matches!(manifest.build.driver, BuildDriver::Cargo(_)),
                "cargo driver is Anemone-only and cannot declare the host target; use the command driver for host Cargo builds"
            );
            for artifact in manifest
                .artifacts
                .iter()
                .filter(|artifact| artifact.supports(&AppTarget::Host))
            {
                anyhow::ensure!(
                    !artifact.path.contains("${TARGET_TRIPLE}"),
                    "host-capable app artifact path '{}' cannot use ${{TARGET_TRIPLE}} because host has no Anemone target triple",
                    artifact.path
                );
            }
        }
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_parses_as_cargo_driver() {
        let content = example_app();
        let app = App::from_str(&content).expect("Failed to parse app.toml");
        assert!(matches!(app.build.driver, BuildDriver::Cargo(_)));
        assert_eq!(
            app.targets,
            [
                AppTarget::Anemone(Arch::RiscV64),
                AppTarget::Anemone(Arch::LoongArch64)
            ]
        );
    }

    #[test]
    fn target_list_is_required_closed_and_unique() {
        let missing = example_app().replacen("targets = [\"riscv64\", \"loongarch64\"]\n", "", 1);
        let error = format!("{:#}", App::from_str(&missing).unwrap_err());
        assert!(error.contains("missing field `targets`"), "{error}");

        let empty = example_app().replacen(
            "targets = [\"riscv64\", \"loongarch64\"]",
            "targets = []",
            1,
        );
        let error = format!("{:#}", App::from_str(&empty).unwrap_err());
        assert!(error.contains("target list must not be empty"), "{error}");

        let duplicate = example_app().replacen(
            "targets = [\"riscv64\", \"loongarch64\"]",
            "targets = [\"riscv64\", \"riscv64\"]",
            1,
        );
        let error = format!("{:#}", App::from_str(&duplicate).unwrap_err());
        assert!(
            error.contains("duplicate app build target 'riscv64'"),
            "{error}"
        );

        let unknown = example_app().replacen("\"loongarch64\"", "\"mips64\"", 1);
        let error = format!("{:#}", App::from_str(&unknown).unwrap_err());
        assert!(
            error.contains("Unsupported app build target: mips64"),
            "{error}"
        );
    }

    #[test]
    fn artifact_list_must_not_be_empty() {
        let empty = r#"
name = "empty"
targets = ["riscv64"]
artifacts = []

[build]
workdir = "."
driver = "source"
"#;
        let error = format!("{:#}", App::from_str(empty).unwrap_err());
        assert!(error.contains("artifact list must not be empty"), "{error}");
    }

    #[test]
    fn artifact_targets_are_closed_and_cover_every_app_target() {
        let valid = r#"
name = "variants"
targets = ["riscv64", "host"]

[build]
workdir = "."
driver = "command"
argv = ["./build.sh"]

[[artifacts]]
path = "out/${TARGET_TRIPLE}/guest"
targets = ["riscv64"]

[[artifacts]]
path = "out/host/native"
targets = ["host"]
"#;
        App::from_str(valid).expect("target-specific artifacts should parse");

        let empty = valid.replacen("targets = [\"host\"]", "targets = []", 1);
        let error = format!("{:#}", App::from_str(&empty).unwrap_err());
        assert!(error.contains("target list must not be empty"), "{error}");

        let duplicate = valid.replacen(
            "targets = [\"host\"]",
            "targets = [\"host\", \"host\"]",
            1,
        );
        let error = format!("{:#}", App::from_str(&duplicate).unwrap_err());
        assert!(error.contains("duplicate build target 'host'"), "{error}");

        let outside = valid.replacen("targets = [\"host\"]", "targets = [\"loongarch64\"]", 1);
        let error = format!("{:#}", App::from_str(&outside).unwrap_err());
        assert!(error.contains("not declared by app 'variants'"), "{error}");

        let uncovered = valid.replace("targets = [\"host\"]", "targets = [\"riscv64\"]");
        let error = format!("{:#}", App::from_str(&uncovered).unwrap_err());
        assert!(error.contains("target 'host' has no artifact"), "{error}");
    }

    #[test]
    fn host_target_requires_a_non_cargo_recipe_without_target_triple_paths() {
        let cargo_host = example_app().replacen(
            "targets = [\"riscv64\", \"loongarch64\"]",
            "targets = [\"riscv64\", \"loongarch64\", \"host\"]",
            1,
        );
        let error = format!("{:#}", App::from_str(&cargo_host).unwrap_err());
        assert!(error.contains("cargo driver is Anemone-only"), "{error}");

        let command_host = cargo_host
            .replacen("driver = \"cargo\"", "driver = \"command\"", 1)
            .replacen("args = [\"build\"]", "argv = [\"cargo\", \"build\"]", 1);
        let error = format!("{:#}", App::from_str(&command_host).unwrap_err());
        assert!(error.contains("cannot use ${TARGET_TRIPLE}"), "{error}");

        let command_host = command_host.replacen(
            "target/${TARGET_TRIPLE}/debug/example",
            "out/${ARCH}/example",
            1,
        );
        App::from_str(&command_host).expect("command driver should admit a host target");
    }

    #[test]
    fn source_driver_is_closed_and_has_no_manifest_args() {
        let source = example_app()
            .replacen("driver = \"cargo\"", "driver = \"source\"", 1)
            .replacen("args = [\"build\"]\n", "", 1);
        let app = App::from_str(&source).expect("source manifest should parse");
        assert!(matches!(app.build.driver, BuildDriver::Source(_)));

        let source_with_args = source.replace(
            "driver = \"source\"",
            "driver = \"source\"\nargs = [\"ignored\"]",
        );
        let error = format!("{:#}", App::from_str(&source_with_args).unwrap_err());
        assert!(error.contains("unknown field `args`"), "{error}");
    }

    #[test]
    fn command_driver_requires_bounded_nonempty_argv() {
        let command = example_app()
            .replacen("driver = \"cargo\"", "driver = \"command\"", 1)
            .replacen("args = [\"build\"]", "argv = [\"./build.sh\"]", 1);
        let app = App::from_str(&command).expect("command manifest should parse");
        assert!(matches!(app.build.driver, BuildDriver::Command(_)));

        let empty = command.replacen("argv = [\"./build.sh\"]", "argv = []", 1);
        let error = format!("{:#}", App::from_str(&empty).unwrap_err());
        assert!(error.contains("argv must not be empty"), "{error}");

        let arguments = std::iter::repeat_n("\"arg\"", MAX_COMMAND_ARGUMENTS + 1)
            .collect::<Vec<_>>()
            .join(", ");
        let oversized = command.replacen(
            "argv = [\"./build.sh\"]",
            &format!("argv = [{arguments}]"),
            1,
        );
        let error = format!("{:#}", App::from_str(&oversized).unwrap_err());
        assert!(error.contains("exceeding the maximum of 256"), "{error}");

        let unknown = command.replacen(
            "argv = [\"./build.sh\"]",
            "argv = [\"./build.sh\"]\nenv = { CC = \"cc\" }",
            1,
        );
        let error = format!("{:#}", App::from_str(&unknown).unwrap_err());
        assert!(error.contains("unknown field `env`"), "{error}");
    }

    fn example_app() -> String {
        std::fs::read_to_string("../../conf/app.toml").expect("failed to read example app manifest")
    }
}
