use std::{path::Path, process::Command};

/// Toolchains and other build-related configuration.
use crate::config::{platform::*, rootfs::FsType};
use crate::tasks::utils::cmd_echo;

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
    pub fn mkfs(&self, root_tree: &Path, output: &Path, use_sudo: bool) -> anyhow::Result<()> {
        match self {
            FsType::Ext4 => {
                let mut command = if use_sudo && !is_effective_root() {
                    let mut command = Command::new("sudo");
                    command.arg(
                        "--preserve-env=LIBGUESTFS_BACKEND,LIBGUESTFS_TRACE,SUPERMIN_KERNEL,SUPERMIN_MODULES",
                    );
                    command.arg("virt-make-fs");
                    command
                } else {
                    Command::new("virt-make-fs")
                };

                command
                    .arg("--type=ext4")
                    .arg("--format=raw")
                    .arg(root_tree)
                    .arg(output);

                cmd_echo(&command);
                let status = command.status()?;
                if !status.success() {
                    anyhow::bail!(
                        "virt-make-fs failed with status: {}. If supermin cannot read /boot, rerun with --sudo or set SUPERMIN_KERNEL and SUPERMIN_MODULES to readable paths.",
                        status
                    );
                }

                // for qemu to run with the generated image without permission issues, we need
                // to make sure the image can be read/write by non-root users.
                //
                // TODO: the owner of the image is still root, which is a bit ugly. refine this
                // later.
                if use_sudo && !is_effective_root() {
                    let mut command = Command::new("sudo");
                    command.arg("chmod").arg("a+rw").arg(output);
                    cmd_echo(&command);
                    let status = command.status()?;
                    if !status.success() {
                        anyhow::bail!("chmod failed with status: {}", status);
                    }
                }

                Ok(())
            },
        }
    }
}

fn is_effective_root() -> bool {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim() == "0")
        .unwrap_or(false)
}

// TODO
