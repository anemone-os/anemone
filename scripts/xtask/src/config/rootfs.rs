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
            if rootfs.fs.size.is_some() {
                anyhow::bail!(
                    "fs.size is not supported with fs.type = 'image'; resize the base image before running rootfs mkfs"
                );
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
pub struct Fs {
    pub fstype: FsType,
    pub base: Option<String>,
    #[serde(rename = "override", default)]
    pub override_dir: Option<String>,
    #[serde(rename = "type", default)]
    pub base_type: BaseType,
    #[serde(default)]
    pub size: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BaseType {
    #[default]
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
    use std::path::{Path, PathBuf};

    #[test]
    fn test_parsing() {
        let rootfs = parse_manifest("../../conf/rootfs/minimal.toml");
        assert_eq!(rootfs.fs.base_type, BaseType::Folder);
        assert!(rootfs.dirs.iter().any(|dir| dir.path == "/dev"));
        assert!(rootfs.dirs.iter().any(|dir| dir.path == "/mnt"));
        println!("{:#?}", rootfs);
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
        assert_eq!(rootfs.fs.size, None);
    }

    #[test]
    fn image_base_rejects_size() {
        let result = Rootfs::from_str(
            r#"
[build]
name = "image-base"
arch = "riscv64"

[fs]
fstype = "ext4"
base = "rootfs.img"
type = "image"
size = "1G"

[init]
path = "/sbin/init"
"#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn pretest_manifests_reference_public_inputs() {
        let rv64 = parse_manifest("../../conf/rootfs/pretest-rv64.toml");
        assert_eq!(rv64.build.name, "pretest-rv64");
        assert_eq!(rv64.build.arch.as_str(), "riscv64");
        assert_manifest_inputs_exist(&rv64);
        assert_mount_dirs(&rv64);

        let la64 = parse_manifest("../../conf/rootfs/pretest-la64.toml");
        assert_eq!(la64.build.name, "pretest-la64");
        assert_eq!(la64.build.arch.as_str(), "loongarch64");
        assert_manifest_inputs_exist(&la64);
        assert_mount_dirs(&la64);
    }

    fn parse_manifest(path: &str) -> Rootfs {
        let content = std::fs::read_to_string(path).expect("Failed to read rootfs.toml");
        toml::from_str(&content).expect("Failed to parse rootfs.toml")
    }

    fn assert_manifest_inputs_exist(rootfs: &Rootfs) {
        if let Some(base) = &rootfs.fs.base {
            assert!(workspace_path(base).is_dir(), "missing rootfs base: {base}");
        }
        if let Some(override_dir) = &rootfs.fs.override_dir {
            assert!(
                workspace_path(override_dir).is_dir(),
                "missing rootfs override: {override_dir}"
            );
        }

        for file in &rootfs.files {
            assert!(
                workspace_path(&file.source).is_file(),
                "missing staged rootfs file: {}",
                file.source
            );
        }
    }

    fn assert_mount_dirs(rootfs: &Rootfs) {
        assert!(rootfs.dirs.iter().any(|dir| dir.path == "/dev"));
        assert!(rootfs.dirs.iter().any(|dir| dir.path == "/mnt"));
    }

    fn workspace_path(path: &str) -> PathBuf {
        Path::new("../..").join(path)
    }
}
