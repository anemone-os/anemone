//! Rootfs manifest.

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::config::platform::Arch;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rootfs {
    pub build: Build,
    pub fs: Fs,
    pub init: Init,
    #[serde(default)]
    pub apps: Vec<App>,
    #[serde(default)]
    pub dirs: Vec<Dir>,
    #[serde(default)]
    pub files: Vec<File>,
}

impl Rootfs {
    pub fn from_str(s: &str) -> anyhow::Result<Self> {
        let rootfs: Self = toml::from_str(s).context("Failed to parse rootfs manifest")?;
        if rootfs.fs.base_type == BaseType::Image {
            if rootfs.fs.base.is_none() {
                anyhow::bail!("fs.type = 'image' requires fs.base");
            }
        }
        Ok(rootfs)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Build {
    pub name: String,
    pub arch: Arch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fs {
    pub fstype: FsType,
    pub base: Option<String>,
    #[serde(rename = "override", default)]
    pub override_dir: Option<String>,
    #[serde(rename = "type")]
    pub base_type: BaseType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BaseType {
    #[serde(rename = "folder")]
    Folder,
    #[serde(rename = "image")]
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FsType {
    #[serde(rename = "ext4")]
    Ext4,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Init {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct App {
    pub name: String,
    pub installed_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dir {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct File {
    pub source: String,
    pub installed_path: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_manifest_parses() {
        let rootfs = parse_manifest("../../conf/rootfs/example.toml");
        assert_eq!(rootfs.build.name, "example");
        assert_eq!(rootfs.fs.base_type, BaseType::Folder);
        assert!(rootfs.dirs.iter().any(|dir| dir.path == "/dev"));
        assert!(rootfs.dirs.iter().any(|dir| dir.path == "/mnt"));
    }

    #[test]
    fn image_base_type_is_explicit() {
        let rootfs = Rootfs::from_str(
            r#"
[build]
name = "image-base"
arch = "riscv64"

[fs]
fstype = "ext4"
base = "rootfs.img"
override = "root-overlay"
type = "image"

[init]
path = "/sbin/init"
"#,
        )
        .unwrap();

        assert_eq!(rootfs.fs.base_type, BaseType::Image);
        assert_eq!(rootfs.fs.base.as_deref(), Some("rootfs.img"));
        assert_eq!(rootfs.fs.override_dir.as_deref(), Some("root-overlay"));
    }

    #[test]
    fn rootfs_type_is_required() {
        let result = Rootfs::from_str(
            r#"
[build]
name = "folder-base"
arch = "riscv64"

[fs]
fstype = "ext4"

[init]
path = "/sbin/init"
"#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn rootfs_size_policy_is_not_configurable() {
        let result = Rootfs::from_str(
            r#"
[build]
name = "folder-base"
arch = "riscv64"

[fs]
fstype = "ext4"
type = "folder"
size = "1G"

[init]
path = "/sbin/init"
"#,
        );

        assert!(result.is_err());
    }

    fn parse_manifest(path: &str) -> Rootfs {
        let content = std::fs::read_to_string(path).expect("Failed to read rootfs.toml");
        Rootfs::from_str(&content).expect("Failed to parse rootfs.toml")
    }
}
