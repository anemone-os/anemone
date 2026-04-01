use std::{path::Path, process::Command};

/// Toolchains and other build-related configuration.
use crate::config::{platform::*, rootfs::FsType};

impl TargetTriple {
    pub fn objdump(&self) -> &'static str {
        "rust-objdump"
    }

    pub fn objcopy(&self) -> &'static str {
        "rust-objcopy"
    }

    /// Produce a path relative to workspace root.
    ///
    /// For an absolute path, convert it to a [std::path::PathBuf] first.
    pub fn spec_json_path(&self) -> &'static Path {
        match self {
            Self::RiscV64UnknownAnemoneElf => {
                Path::new("conf/arch/riscv64/riscv64-unknown-anemone-elf.json")
            },
            Self::LoongArch64UnknownAnemoneElf => {
                Path::new("conf/arch/loongarch64/loongarch64-unknown-anemone-elf.json")
            },
        }
    }
}

impl FsType {
    fn estimate_tree_size(&self, root_tree: &Path) -> anyhow::Result<usize> {
        fn walk_dir(dir: &Path) -> anyhow::Result<usize> {
            let mut total_size = 0;
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let metadata = entry.metadata()?;
                if metadata.is_file() {
                    total_size += metadata.len() as usize;
                } else if metadata.is_dir() {
                    total_size += walk_dir(&entry.path())?;
                }
            }
            Ok(total_size)
        }
        walk_dir(root_tree)
    }

    pub fn mkfs(&self, root_tree: &Path, output: &Path) -> anyhow::Result<()> {
        match self {
            FsType::Ext4 => {
                // mke2fs -t ext4 -d root_tree output estimate_size

                let estimate_bytes = {
                    let bytes = self.estimate_tree_size(root_tree)?;
                    // add 20% overhead for ext4 metadata, which should be enough
                    let bytes = bytes + bytes / 5;
                    // add 20MB minimum to avoid mke2fs complaining about small filesystems
                    let bytes = bytes + 20 * 1024 * 1024;
                    // round up to nearest 4K
                    let bytes = (bytes + 4095) & !4095;
                    bytes
                };

                let status = Command::new("mke2fs")
                    .arg("-t")
                    .arg("ext4")
                    .arg("-d")
                    .arg(root_tree)
                    .arg(output)
                    .arg(estimate_bytes.to_string())
                    .status()?;
                if !status.success() {
                    anyhow::bail!("mke2fs failed with status: {}", status);
                }

                Ok(())
            },
        }
    }
}

// TODO
