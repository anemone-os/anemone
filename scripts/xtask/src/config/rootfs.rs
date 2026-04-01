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
    pub files: Vec<File>,
}

impl Rootfs {
    pub fn from_str(s: &str) -> anyhow::Result<Self> {
        let rootfs = toml::from_str(s).context("Failed to parse rootfs manifest")?;
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FsType {
    #[serde(rename = "ext4")]
    Ext4,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Init {
    pub path: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct App {
    pub name: String,
    pub installed_dir: String,
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
    fn test_parsing() {
        let content = std::fs::read_to_string("../../conf/rootfs/minimal.toml")
            .expect("Failed to read rootfs.toml");
        let rootfs: Rootfs = toml::from_str(&content).expect("Failed to parse rootfs.toml");
        println!("{:#?}", rootfs);
    }
}
