//! SystemTarget-owned boot and root selection.
//!
//! The Stage 1B cutover removed the legacy Platform root fields. Production
//! build resolution must use this value and must not recreate a Platform fallback.

use serde::Deserialize;

use super::reference::PlatformRef;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub platform: PlatformRef,
    pub root: Root,
    #[serde(rename = "initial-program")]
    pub initial_program: InitialProgramSource,
}

impl Config {
    pub fn from_str(content: &str) -> anyhow::Result<Self> {
        let config: Self = toml::from_str(content)?;
        config.root.validate()?;
        Ok(config)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Root {
    pub fstype: String,
    pub source: RootSource,
}

impl Root {
    fn validate(&self) -> anyhow::Result<()> {
        if self.fstype.is_empty() {
            anyhow::bail!("system target root filesystem type must not be empty");
        }
        if let RootSource::Block { path } = &self.source
            && path.is_empty()
        {
            anyhow::bail!("system target block root path must not be empty");
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RootSource {
    Block { path: String },
    Pseudo,
}

impl RootSource {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Block { .. } => "block",
            Self::Pseudo => "pseudo",
        }
    }

    pub fn path(&self) -> Option<&str> {
        match self {
            Self::Block { path } => Some(path),
            Self::Pseudo => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum InitialProgramSource {
    RootfsEntry,
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_TARGET: &str = r#"
platform = "qemu-virt-rv64"

[root]
fstype = "ext4"
source = { type = "block", path = "vda" }

[initial-program]
type = "rootfs-entry"
"#;

    #[test]
    fn parses_minimal_rootfs_entry_target() {
        let config = Config::from_str(VALID_TARGET).unwrap();
        assert_eq!(config.platform.as_str(), "qemu-virt-rv64");
        assert_eq!(config.root.fstype, "ext4");
        assert!(matches!(config.root.source, RootSource::Block { .. }));
        assert!(matches!(
            config.initial_program,
            InitialProgramSource::RootfsEntry
        ));
    }

    #[test]
    fn rejects_unsupported_initial_program_tag() {
        let content = VALID_TARGET.replace("rootfs-entry", "embedded-app");
        assert!(Config::from_str(&content).is_err());
    }

    #[test]
    fn rejects_invalid_root_source() {
        let empty_fstype = VALID_TARGET.replace("fstype = \"ext4\"", "fstype = \"\"");
        assert!(Config::from_str(&empty_fstype).is_err());

        let empty_block_path = VALID_TARGET.replace("path = \"vda\"", "path = \"\"");
        assert!(Config::from_str(&empty_block_path).is_err());
    }

    #[test]
    fn rejects_fields_owned_by_other_layers() {
        for field in [
            "preset = \"dev\"",
            "profile = \"release\"",
            "qemu = {}",
            "outputs = []",
        ] {
            let content = VALID_TARGET.replacen(
                "platform = \"qemu-virt-rv64\"",
                &format!("platform = \"qemu-virt-rv64\"\n{field}"),
                1,
            );
            assert!(Config::from_str(&content).is_err(), "{field}");
        }
    }
}
