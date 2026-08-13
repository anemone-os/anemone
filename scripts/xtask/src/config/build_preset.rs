use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::reference::{BuildPresetRef, KernelConfigRef, SystemTargetRef};

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct BuildPreset {
    pub target: SystemTargetRef,
    #[serde(rename = "kernel-config")]
    pub kernel_config: KernelConfigRef,
    pub profile: CargoProfile,
}

impl BuildPreset {
    pub fn from_str(content: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(content)?)
    }
}

#[derive(Deserialize, Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CargoProfile {
    Dev,
    Release,
}

impl CargoProfile {
    pub fn as_cargo_arg(&self) -> &'static [&'static str] {
        match self {
            Self::Dev => &["--profile", "dev"],
            Self::Release => &["--release"],
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Release => "release",
        }
    }
}

impl FromStr for CargoProfile {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "dev" => Ok(Self::Dev),
            "release" => Ok(Self::Release),
            _ => anyhow::bail!("unsupported kernel Cargo profile `{value}`"),
        }
    }
}

#[cfg(test)]
pub(crate) const TEST_BUILD_PRESET: &str = r#"
target = "example"
kernel-config = "kconfig"
profile = "release"
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_closed_build_preset() {
        let preset = BuildPreset::from_str(TEST_BUILD_PRESET).unwrap();
        assert_eq!(preset.target.as_str(), "example");
        assert_eq!(preset.kernel_config.to_string(), "kconfig");
        assert_eq!(preset.profile, CargoProfile::Release);
        assert_eq!(CargoProfile::Dev.as_cargo_arg(), ["--profile", "dev"]);
        assert_eq!(CargoProfile::Release.as_cargo_arg(), ["--release"]);
    }

    #[test]
    fn rejects_non_preset_fields_and_profiles() {
        let valid = TEST_BUILD_PRESET;
        for invalid in [
            valid.replace("profile = \"release\"", "profile = \"other\""),
            format!("{valid}\ndisasm = true\n"),
            format!("{valid}\nqemu = \"path\"\n"),
            format!("{valid}\nbind = []\n"),
        ] {
            assert!(BuildPreset::from_str(&invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn build_preset_ref_accepts_bounded_workspace_paths() {
        assert_eq!(BuildPresetRef::new("example").unwrap().as_str(), "example");
        assert_eq!(
            BuildPresetRef::new("local/preset.toml").unwrap().as_str(),
            "local/preset.toml"
        );
        assert!(BuildPresetRef::new("../preset").is_err());
    }

    #[test]
    fn repository_example_build_preset_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../conf/build-presets/example.toml");
        let content = std::fs::read_to_string(path).expect("failed to read example build preset");
        BuildPreset::from_str(&content).expect("repository example build preset must parse");
    }
}
