//! Manifest for Nemophila modules under `nemophila/modules`.

use std::path::{Component, Path};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NemophilaModule {
    pub name: String,
    pub build: Build,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Build {
    pub workdir: String,
    pub driver: ModuleBuildDriver,
    pub manifest: String,
    pub package: String,
    pub artifact: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ModuleBuildDriver {
    Cargo,
}

impl NemophilaModule {
    pub fn from_str(content: &str) -> anyhow::Result<Self> {
        let manifest: Self =
            toml::from_str(content).with_context(|| "failed to parse Nemophila module manifest")?;
        validate_identity(&manifest.name)?;
        validate_relative_path("build.workdir", &manifest.build.workdir)?;
        validate_relative_path("build.manifest", &manifest.build.manifest)?;
        validate_package(&manifest.build.package)?;
        validate_artifact(&manifest.build.artifact)?;
        Ok(manifest)
    }
}

pub(crate) fn validate_identity(identity: &str) -> anyhow::Result<()> {
    if identity.is_empty()
        || identity.starts_with('-')
        || identity.ends_with('-')
        || !identity
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        bail!("module identity '{identity}' must be lower kebab-case");
    }
    Ok(())
}

fn validate_package(package: &str) -> anyhow::Result<()> {
    if package.is_empty()
        || !package
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("Cargo package '{package}' contains unsupported characters");
    }
    Ok(())
}

fn validate_artifact(artifact: &str) -> anyhow::Result<()> {
    let path = Path::new(artifact);
    validate_relative_path("build.artifact", artifact)?;
    if path
        .parent()
        .is_some_and(|parent| !parent.as_os_str().is_empty())
        || path.extension().and_then(|extension| extension.to_str()) != Some("wasm")
    {
        bail!("module artifact '{artifact}' must be a single .wasm file name");
    }
    Ok(())
}

fn validate_relative_path(field: &str, value: &str) -> anyhow::Result<()> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("{field} must be a non-empty module-relative path");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"
name = "clone-observer"

[build]
workdir = "."
driver = "cargo"
manifest = "Cargo.toml"
package = "nemophila-clone-observer"
artifact = "nemophila_clone_observer.wasm"
"#;

    #[test]
    fn cargo_module_manifest_is_closed_and_architecture_free() {
        let manifest = NemophilaModule::from_str(MANIFEST).unwrap();
        assert_eq!(manifest.name, "clone-observer");
        assert_eq!(manifest.build.driver, ModuleBuildDriver::Cargo);

        for extra in [
            "target = \"riscv64\"",
            "architecture = \"loongarch64\"",
            "argv = [\"build.sh\"]",
        ] {
            let invalid = format!("{MANIFEST}\n{extra}\n");
            let error = format!("{:#}", NemophilaModule::from_str(&invalid).unwrap_err());
            assert!(error.contains("unknown field"), "{error}");
        }
    }

    #[test]
    fn identity_paths_package_and_candidate_are_validated() {
        for (old, new, expected) in [
            (
                "name = \"clone-observer\"",
                "name = \"Clone Observer\"",
                "lower kebab-case",
            ),
            (
                "workdir = \".\"",
                "workdir = \"../outside\"",
                "module-relative path",
            ),
            (
                "package = \"nemophila-clone-observer\"",
                "package = \"bad package\"",
                "unsupported characters",
            ),
            (
                "artifact = \"nemophila_clone_observer.wasm\"",
                "artifact = \"nested/module.wasm\"",
                "single .wasm file name",
            ),
        ] {
            let invalid = MANIFEST.replacen(old, new, 1);
            let error = format!("{:#}", NemophilaModule::from_str(&invalid).unwrap_err());
            assert!(error.contains(expected), "{error}");
        }
    }

    #[test]
    fn repository_module_manifest_parses() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../nemophila/modules/clone-observer/module.toml");
        let content = std::fs::read_to_string(path).unwrap();
        NemophilaModule::from_str(&content).unwrap();
    }
}
