use std::{path::Path, process::Command};

/// Toolchains and other build-related configuration.
use crate::config::{platform::*, rootfs::FsType};
use crate::tasks::utils::{cmd_echo, log_progress};

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
    pub fn mkfs(
        &self,
        root_tree: &Path,
        output: &Path,
        size: Option<&str>,
        use_sudo: bool,
    ) -> anyhow::Result<()> {
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

                command.arg("--type=ext4").arg("--format=raw");
                if let Some(size) = size {
                    command.arg(format!("--size={size}"));
                }
                command.arg(root_tree).arg(output);

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

    pub fn mkfs_from_image(
        &self,
        base_image: &Path,
        override_tree: Option<&Path>,
        generated_tree: &Path,
        output: &Path,
        use_sudo: bool,
    ) -> anyhow::Result<()> {
        match self {
            FsType::Ext4 => {
                let mut command = Command::new("cp");
                command.arg(base_image).arg(output);
                run_command(&mut command, "failed to copy rootfs base image")?;

                if let Some(override_tree) = override_tree {
                    copy_tree_into_image(override_tree, output, use_sudo, "rootfs override")?;
                }
                copy_tree_into_image(
                    generated_tree,
                    output,
                    use_sudo,
                    "staged files and generated metadata",
                )?;

                ensure_user_writable(output, use_sudo)
            },
        }
    }
}

fn copy_tree_into_image(
    tree: &Path,
    output: &Path,
    use_sudo: bool,
    description: &str,
) -> anyhow::Result<()> {
    let mut entries = std::fs::read_dir(tree)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();
    if entries.is_empty() {
        return Ok(());
    }

    log_progress(
        "MKFS",
        &format!("Copying {description} from '{}'", tree.display()),
    );

    let mut command = libguestfs_command("virt-copy-in", use_sudo);
    // virt-copy-in is a guestfish wrapper. Without --pipe-error, a failed
    // tar-in can be printed as an error while guestfish exits successfully.
    // Pass top-level entries so recursive contents land at / rather than an
    // extra directory named after the source tree.
    command
        .arg("--pipe-error")
        .arg("--format=raw")
        .arg("-a")
        .arg(output);
    command.args(&entries).arg("/");
    run_command(
        &mut command,
        &format!("virt-copy-in failed for {description}"),
    )
}

fn libguestfs_command(program: &str, use_sudo: bool) -> Command {
    if use_sudo && !is_effective_root() {
        let mut command = Command::new("sudo");
        command.arg(
            "--preserve-env=LIBGUESTFS_BACKEND,LIBGUESTFS_TRACE,SUPERMIN_KERNEL,SUPERMIN_MODULES",
        );
        command.arg(program);
        command
    } else {
        Command::new(program)
    }
}

fn run_command(command: &mut Command, failure: &str) -> anyhow::Result<()> {
    cmd_echo(command);
    let status = command.status()?;
    if !status.success() {
        anyhow::bail!("{} with status: {}", failure, status);
    }
    Ok(())
}

fn ensure_user_writable(output: &Path, use_sudo: bool) -> anyhow::Result<()> {
    if use_sudo && !is_effective_root() {
        let mut command = Command::new("sudo");
        command.arg("chmod").arg("a+rw").arg(output);
        run_command(&mut command, "chmod failed")?;
    }
    Ok(())
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
