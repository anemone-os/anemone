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
mod tests {
    use super::*;

    #[test]
    fn parses_closed_build_preset() {
        let preset = BuildPreset::from_str(&example_preset()).unwrap();
        assert_eq!(preset.target.as_str(), "example");
        assert_eq!(preset.kernel_config.to_string(), "conf/.defconfig");
        assert_eq!(preset.profile, CargoProfile::Release);
        assert_eq!(CargoProfile::Dev.as_cargo_arg(), ["--profile", "dev"]);
        assert_eq!(CargoProfile::Release.as_cargo_arg(), ["--release"]);
    }

    #[test]
    fn rejects_non_preset_fields_and_profiles() {
        let valid = example_preset();
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
    fn build_preset_ref_remains_filename_identity() {
        assert!(BuildPresetRef::new("example").is_ok());
        assert!(BuildPresetRef::new("../preset").is_err());
    }

    fn example_preset() -> String {
        std::fs::read_to_string("../../conf/build-presets/example.toml")
            .expect("failed to read example build preset")
    }
}
