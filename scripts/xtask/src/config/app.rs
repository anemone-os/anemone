//! Manifest for applications under anemone-apps.

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// Maximum final Command argv length, including the program and CLI extras.
///
/// Command is trusted repository build code, but rejecting accidental argv
/// explosions here gives manifests a stable failure boundary before host exec.
pub const MAX_COMMAND_ARGUMENTS: usize = 256;

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
}

impl App {
    pub fn from_str(content: &str) -> anyhow::Result<Self> {
        let manifest: App = toml::from_str(content)
            .with_context(|| "Failed to parse app manifest from string content")?;
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
