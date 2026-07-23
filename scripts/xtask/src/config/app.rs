//! Manifest for applications under anemone-apps.

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct App {
    pub name: String,
    pub build: Build,
    pub artifacts: Vec<Artifact>,
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
    #[serde(rename = "source")]
    Source(SourceBuild),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoBuild {
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceBuild {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub path: String,
}

impl App {
    pub fn from_str(content: &str) -> anyhow::Result<Self> {
        let manifest: App = toml::from_str(content)
            .with_context(|| "Failed to parse app manifest from string content")?;
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_parses_as_cargo_driver() {
        let content =
            std::fs::read_to_string("../../conf/app.toml").expect("Failed to read app.toml");
        let app = App::from_str(&content).expect("Failed to parse app.toml");
        assert!(matches!(app.build.driver, BuildDriver::Cargo(_)));
    }

    #[test]
    fn source_driver_is_closed_and_has_no_manifest_args() {
        let source = r#"
name = "prebuilt"

[build]
workdir = "."
driver = "source"

[[artifacts]]
path = "bin/${ARCH}/prebuilt"
"#;
        let app = App::from_str(source).expect("source manifest should parse");
        assert!(matches!(app.build.driver, BuildDriver::Source(_)));

        let source_with_args = source.replace(
            "driver = \"source\"",
            "driver = \"source\"\nargs = [\"ignored\"]",
        );
        let error = format!("{:#}", App::from_str(&source_with_args).unwrap_err());
        assert!(error.contains("unknown field `args`"), "{error}");
    }

    #[test]
    fn tracked_app_manifests_remain_valid_cargo_drivers() {
        for entry in std::fs::read_dir("../../anemone-apps").unwrap() {
            let entry = entry.unwrap();
            let manifest = entry.path().join("app.toml");
            if !manifest.is_file() {
                continue;
            }

            let content = std::fs::read_to_string(&manifest).unwrap();
            let app = App::from_str(&content)
                .unwrap_or_else(|error| panic!("{}: {error:#}", manifest.display()));
            assert!(
                matches!(app.build.driver, BuildDriver::Cargo(_)),
                "tracked app '{}' unexpectedly changed driver",
                app.name
            );
        }
    }
}
