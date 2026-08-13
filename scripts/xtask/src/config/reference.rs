use std::{
    fmt,
    path::{Component, Path, PathBuf},
};

use anyhow::Context;
use serde::{Deserialize, Deserializer};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AppRef(String);

impl AppRef {
    pub fn new(value: &str) -> anyhow::Result<Self> {
        validate_slug("app", value)?;
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AppRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<'de> Deserialize<'de> for AppRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ConfigFileRef {
    path: PathBuf,
    // This is protocol state rather than derived path metadata: `name` and
    // `./name` normalize alike, but the latter must bypass canonical lookup.
    lookup: ConfigLookup,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum ConfigLookup {
    CanonicalFirst,
    WorkspaceOnly,
}

impl ConfigFileRef {
    fn new(kind: &str, value: impl AsRef<Path>) -> anyhow::Result<Self> {
        let value = value.as_ref();
        let mut components = value.components();
        let single_plain_name =
            matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
        let path = normalize_workspace_relative(kind, value)?;
        let lookup = if single_plain_name {
            let name = path
                .to_str()
                .context(format!("{kind} reference must be valid UTF-8"))?;
            if validate_slug(kind, name).is_ok() {
                ConfigLookup::CanonicalFirst
            } else {
                ConfigLookup::WorkspaceOnly
            }
        } else {
            ConfigLookup::WorkspaceOnly
        };
        Ok(Self { path, lookup })
    }

    fn as_path(&self) -> &Path {
        &self.path
    }

    fn as_str(&self) -> &str {
        self.path
            .to_str()
            .expect("ConfigFileRef construction validates UTF-8")
    }

    fn canonical_name(&self) -> Option<&str> {
        (self.lookup == ConfigLookup::CanonicalFirst).then(|| self.as_str())
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BuildPresetRef(ConfigFileRef);

impl BuildPresetRef {
    pub fn new(value: impl AsRef<Path>) -> anyhow::Result<Self> {
        ConfigFileRef::new("build preset", value).map(Self)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub(super) fn as_path(&self) -> &Path {
        self.0.as_path()
    }

    pub(super) fn canonical_name(&self) -> Option<&str> {
        self.0.canonical_name()
    }
}

impl fmt::Display for BuildPresetRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.as_path().display().fmt(formatter)
    }
}

impl<'de> Deserialize<'de> for BuildPresetRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = PathBuf::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SystemTargetRef(ConfigFileRef);

impl SystemTargetRef {
    pub fn new(value: impl AsRef<Path>) -> anyhow::Result<Self> {
        ConfigFileRef::new("system target", value).map(Self)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub(super) fn as_path(&self) -> &Path {
        self.0.as_path()
    }

    pub(super) fn canonical_name(&self) -> Option<&str> {
        self.0.canonical_name()
    }
}

impl fmt::Display for SystemTargetRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.as_path().display().fmt(formatter)
    }
}

impl<'de> Deserialize<'de> for SystemTargetRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = PathBuf::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PlatformRef(ConfigFileRef);

impl PlatformRef {
    pub fn new(value: impl AsRef<Path>) -> anyhow::Result<Self> {
        ConfigFileRef::new("platform", value).map(Self)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub(super) fn as_path(&self) -> &Path {
        self.0.as_path()
    }

    pub(super) fn canonical_name(&self) -> Option<&str> {
        self.0.canonical_name()
    }
}

impl fmt::Display for PlatformRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.as_path().display().fmt(formatter)
    }
}

impl<'de> Deserialize<'de> for PlatformRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = PathBuf::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct KernelConfigRef(PathBuf);

impl KernelConfigRef {
    pub fn new(value: impl AsRef<Path>) -> anyhow::Result<Self> {
        normalize_workspace_relative("kernel config", value).map(Self)
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

fn normalize_workspace_relative(kind: &str, value: impl AsRef<Path>) -> anyhow::Result<PathBuf> {
    let value = value.as_ref();
    if value.as_os_str().is_empty() {
        anyhow::bail!("{kind} reference must not be empty");
    }

    let mut normalized = PathBuf::new();
    for component in value.components() {
        match component {
            Component::Normal(segment) => normalized.push(segment),
            Component::CurDir => {},
            Component::ParentDir => {
                if !normalized.pop() {
                    anyhow::bail!(
                        "{kind} reference must not escape the workspace: {}",
                        value.display()
                    );
                }
            },
            Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!(
                    "{kind} reference must be workspace-relative: {}",
                    value.display()
                )
            },
        }
    }

    if normalized.as_os_str().is_empty() {
        anyhow::bail!("{kind} reference must name a file");
    }
    normalized
        .to_str()
        .with_context(|| format!("{kind} reference must be valid UTF-8"))?;
    Ok(normalized)
}

impl fmt::Display for KernelConfigRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.display().fmt(formatter)
    }
}

impl<'de> Deserialize<'de> for KernelConfigRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = PathBuf::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

pub(super) fn validate_slug(kind: &str, value: &str) -> anyhow::Result<()> {
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
    fn config_file_references_preserve_canonical_and_path_modes() {
        for valid in ["example", "test-target", "0-test", "a-"] {
            assert!(AppRef::new(valid).is_ok(), "{valid}");
            assert_eq!(
                BuildPresetRef::new(valid).unwrap().canonical_name(),
                Some(valid)
            );
            assert_eq!(
                SystemTargetRef::new(valid).unwrap().canonical_name(),
                Some(valid)
            );
            assert_eq!(
                PlatformRef::new(valid).unwrap().canonical_name(),
                Some(valid)
            );
        }
        for valid_path in [
            "-target",
            "Target",
            "target_name",
            "target.toml",
            "local/target",
        ] {
            assert!(AppRef::new(valid_path).is_err(), "{valid_path}");
            assert!(
                BuildPresetRef::new(valid_path)
                    .unwrap()
                    .canonical_name()
                    .is_none(),
                "{valid_path}"
            );
            assert!(
                SystemTargetRef::new(valid_path)
                    .unwrap()
                    .canonical_name()
                    .is_none(),
                "{valid_path}"
            );
            assert!(
                PlatformRef::new(valid_path)
                    .unwrap()
                    .canonical_name()
                    .is_none(),
                "{valid_path}"
            );
        }
        let forced = BuildPresetRef::new("./example").unwrap();
        assert_eq!(forced.as_path(), Path::new("example"));
        assert!(forced.canonical_name().is_none());

        for invalid in ["", ".", "../target", "/target"] {
            assert!(AppRef::new(invalid).is_err(), "{invalid}");
            assert!(BuildPresetRef::new(invalid).is_err(), "{invalid}");
            assert!(SystemTargetRef::new(invalid).is_err(), "{invalid}");
            assert!(PlatformRef::new(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn kernel_config_references_are_normalized_and_bounded() {
        let reference = KernelConfigRef::new("./conf/./kconfs/default.toml").unwrap();
        assert_eq!(reference.as_path(), Path::new("conf/kconfs/default.toml"));
        let reference = KernelConfigRef::new("conf/../kconfig").unwrap();
        assert_eq!(reference.as_path(), Path::new("kconfig"));

        assert!(KernelConfigRef::new("").is_err());
        assert!(KernelConfigRef::new(".").is_err());
        assert!(KernelConfigRef::new("../kconfig").is_err());
        assert!(KernelConfigRef::new("/tmp/kconfig").is_err());
    }
}
