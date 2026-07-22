use std::{
    fmt,
    path::{Component, Path, PathBuf},
};

use anyhow::Context;
use serde::{Deserialize, Deserializer};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SystemTargetRef(String);

impl SystemTargetRef {
    pub fn new(value: &str) -> anyhow::Result<Self> {
        validate_slug("system target", value)?;
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SystemTargetRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<'de> Deserialize<'de> for SystemTargetRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PlatformRef(String);

impl PlatformRef {
    pub fn new(value: &str) -> anyhow::Result<Self> {
        validate_slug("platform", value)?;
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PlatformRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<'de> Deserialize<'de> for PlatformRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct KernelConfigRef(PathBuf);

impl KernelConfigRef {
    pub fn new(value: impl AsRef<Path>) -> anyhow::Result<Self> {
        let value = value.as_ref();
        if value.as_os_str().is_empty() {
            anyhow::bail!("kernel config reference must not be empty");
        }

        let mut normalized = PathBuf::new();
        for component in value.components() {
            match component {
                Component::Normal(segment) => normalized.push(segment),
                Component::CurDir => {}
                Component::ParentDir => {
                    if !normalized.pop() {
                        anyhow::bail!(
                            "kernel config reference must not escape the workspace: {}",
                            value.display()
                        );
                    }
                }
                Component::RootDir | Component::Prefix(_) => {
                    anyhow::bail!(
                        "kernel config reference must be workspace-relative: {}",
                        value.display()
                    )
                }
            }
        }

        if normalized.as_os_str().is_empty() {
            anyhow::bail!("kernel config reference must name a file");
        }
        normalized
            .to_str()
            .context("kernel config reference must be valid UTF-8")?;
        Ok(Self(normalized))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl fmt::Display for KernelConfigRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.display().fmt(formatter)
    }
}

fn validate_slug(kind: &str, value: &str) -> anyhow::Result<()> {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        anyhow::bail!("{kind} reference must not be empty");
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        anyhow::bail!("invalid {kind} reference `{value}`");
    }
    if !bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-') {
        anyhow::bail!("invalid {kind} reference `{value}`");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_references_are_strict() {
        for valid in ["qemu-virt-rv64", "visionfive2-rv64", "0-test", "a-"] {
            assert!(SystemTargetRef::new(valid).is_ok(), "{valid}");
            assert!(PlatformRef::new(valid).is_ok(), "{valid}");
        }
        for invalid in [
            "",
            "-target",
            "Target",
            "target_name",
            "target.toml",
            "anemoneImage-rv64.bin",
            "../target",
            "/target",
        ] {
            assert!(SystemTargetRef::new(invalid).is_err(), "{invalid}");
            assert!(PlatformRef::new(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn kernel_config_references_are_normalized_and_bounded() {
        let reference = KernelConfigRef::new("./conf/./.defconfig").unwrap();
        assert_eq!(reference.as_path(), Path::new("conf/.defconfig"));
        let reference = KernelConfigRef::new("conf/../kconfig").unwrap();
        assert_eq!(reference.as_path(), Path::new("kconfig"));

        assert!(KernelConfigRef::new("").is_err());
        assert!(KernelConfigRef::new(".").is_err());
        assert!(KernelConfigRef::new("../kconfig").is_err());
        assert!(KernelConfigRef::new("/tmp/kconfig").is_err());
    }
}
