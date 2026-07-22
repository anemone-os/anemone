use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::Context;

use crate::{
    config::platform::{Arch, Uboot},
    log_progress,
    tasks::utils::cmd_echo,
};

const KERNEL_ELF: &str = "build/anemone.elf";
const BUILD_DIR: &str = "build";

pub(super) fn build_uboot_image(arch: &Arch, uboot: Option<&Uboot>) -> anyhow::Result<()> {
    let Some(uboot) = uboot else {
        return Ok(());
    };

    UbootPostLink::new(arch, uboot, Path::new(KERNEL_ELF), Path::new(BUILD_DIR)).execute()
}

struct UbootPostLink<'a> {
    objcopy: &'static str,
    uboot: &'a Uboot,
    kernel_elf: PathBuf,
    raw_output: PathBuf,
    legacy_output: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PostLinkStep {
    Objcopy,
    Mkimage,
}

impl PostLinkStep {
    fn action(self) -> &'static str {
        match self {
            Self::Objcopy => "export raw kernel binary",
            Self::Mkimage => "build U-Boot legacy image",
        }
    }
}

impl<'a> UbootPostLink<'a> {
    fn new(arch: &Arch, uboot: &'a Uboot, kernel_elf: &Path, output_dir: &Path) -> Self {
        let legacy_output = output_dir.join(&uboot.filename);
        let raw_output = PathBuf::from(format!("{}.bin", legacy_output.display()));
        Self {
            objcopy: arch.target_triple().objcopy(),
            uboot,
            kernel_elf: kernel_elf.to_owned(),
            raw_output,
            legacy_output,
        }
    }

    fn execute(&self) -> anyhow::Result<()> {
        self.execute_with(run_command)
    }

    fn execute_with(
        &self,
        mut run: impl FnMut(PostLinkStep, &mut Command) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        if let Some(parent) = self.legacy_output.parent() {
            fs::create_dir_all(parent)?;
        }

        // A failed post-link must not leave a previous image looking like this build's output.
        self.remove_outputs()?;
        let result = [PostLinkStep::Objcopy, PostLinkStep::Mkimage]
            .into_iter()
            .try_for_each(|step| {
                let mut command = self.command(step);
                run(step, &mut command)
            });

        if let Err(error) = result {
            return match self.remove_outputs() {
                Ok(()) => Err(error),
                Err(cleanup_error) => Err(error.context(format!(
                    "failed to clean partial U-Boot outputs: {cleanup_error:#}"
                ))),
            };
        }

        Ok(())
    }

    fn command(&self, step: PostLinkStep) -> Command {
        match step {
            PostLinkStep::Objcopy => {
                log_progress!(
                    "UBOOT",
                    &format!(
                        "Generating raw kernel binary '{}'",
                        self.raw_output.display()
                    )
                );
                let mut command = Command::new(self.objcopy);
                command
                    .arg("-O")
                    .arg("binary")
                    .arg(&self.kernel_elf)
                    .arg(&self.raw_output);
                command
            },
            PostLinkStep::Mkimage => {
                log_progress!(
                    "UBOOT",
                    &format!(
                        "Generating U-Boot image '{}'",
                        self.legacy_output.display()
                    )
                );
                let mut command = Command::new("mkimage");
                command
                    .arg("-A")
                    .arg(&self.uboot.arch)
                    .arg("-O")
                    .arg(&self.uboot.os_type)
                    .arg("-T")
                    .arg(&self.uboot.image_type)
                    .arg("-C")
                    .arg(&self.uboot.compression)
                    .arg("-a")
                    .arg(format!("0x{:x}", self.uboot.load_addr))
                    .arg("-e")
                    .arg(format!("0x{:x}", self.uboot.entry))
                    .arg("-n")
                    .arg(&self.uboot.name)
                    .arg("-d")
                    .arg(&self.raw_output)
                    .arg(&self.legacy_output);
                command
            },
        }
    }

    fn remove_outputs(&self) -> anyhow::Result<()> {
        for output in [&self.raw_output, &self.legacy_output] {
            match fs::remove_file(output) {
                Ok(()) => {},
                Err(error) if error.kind() == ErrorKind::NotFound => {},
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to remove '{}'", output.display()));
                },
            }
        }
        Ok(())
    }
}

fn run_command(step: PostLinkStep, command: &mut Command) -> anyhow::Result<()> {
    cmd_echo(command);
    let program = command.get_program().to_string_lossy().into_owned();
    let status = command.status().with_context(|| {
        format!(
            "U-Boot post-link action '{}' failed to execute '{}'",
            step.action(),
            program
        )
    })?;
    if !status.success() {
        anyhow::bail!(
            "U-Boot post-link action '{}' failed: '{}' exited with status {}",
            step.action(),
            program,
            status
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, time::SystemTime};

    use super::*;

    fn uboot() -> Uboot {
        Uboot {
            arch: "riscv".to_string(),
            os_type: "linux".to_string(),
            image_type: "kernel".to_string(),
            compression: "none".to_string(),
            load_addr: 0x80200000,
            entry: 0x80200000,
            name: "Anemone OS for RISC-V".to_string(),
            filename: "anemoneImage-rv64".to_string(),
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "anemone-xtask-kernel-output-{}-{name}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn args(command: &Command) -> Vec<OsString> {
        command.get_args().map(OsString::from).collect()
    }

    #[test]
    fn no_uboot_config_skips_post_link() {
        build_uboot_image(&Arch::RiscV64, None).unwrap();
    }

    #[test]
    fn constructs_objcopy_then_mkimage_with_platform_fields() {
        let output_dir = temp_dir("commands");
        let uboot = uboot();
        let plan = UbootPostLink::new(
            &Arch::RiscV64,
            &uboot,
            Path::new("build/anemone.elf"),
            &output_dir,
        );
        let mut commands = Vec::new();
        plan.execute_with(|step, command| {
            commands.push((
                step,
                command.get_program().to_os_string(),
                args(command),
            ));
            Ok(())
        })
        .unwrap();

        let raw = output_dir.join("anemoneImage-rv64.bin");
        let legacy = output_dir.join("anemoneImage-rv64");
        assert_eq!(commands[0].0, PostLinkStep::Objcopy);
        assert_eq!(commands[0].1, "rust-objcopy");
        assert_eq!(
            commands[0].2,
            ["-O", "binary", "build/anemone.elf"]
                .into_iter()
                .map(OsString::from)
                .chain([raw.as_os_str().to_owned()])
                .collect::<Vec<_>>()
        );
        assert_eq!(commands[1].0, PostLinkStep::Mkimage);
        assert_eq!(commands[1].1, "mkimage");
        assert_eq!(
            commands[1].2,
            [
                "-A",
                "riscv",
                "-O",
                "linux",
                "-T",
                "kernel",
                "-C",
                "none",
                "-a",
                "0x80200000",
                "-e",
                "0x80200000",
                "-n",
                "Anemone OS for RISC-V",
                "-d",
            ]
            .into_iter()
            .map(OsString::from)
            .chain([raw.into_os_string(), legacy.into_os_string()])
            .collect::<Vec<_>>()
        );
        fs::remove_dir_all(output_dir).unwrap();
    }

    #[test]
    fn objcopy_failure_short_circuits_and_removes_partial_outputs() {
        let output_dir = temp_dir("objcopy-failure");
        let uboot = uboot();
        let plan = UbootPostLink::new(
            &Arch::RiscV64,
            &uboot,
            Path::new("build/anemone.elf"),
            &output_dir,
        );
        fs::write(&plan.raw_output, b"stale raw").unwrap();
        fs::write(&plan.legacy_output, b"stale image").unwrap();
        let mut steps = Vec::new();
        let error = plan
            .execute_with(|step, _| {
                steps.push(step);
                fs::write(&plan.raw_output, b"partial raw").unwrap();
                anyhow::bail!("objcopy fixture failed")
            })
            .unwrap_err();

        assert!(error.to_string().contains("objcopy fixture failed"));
        assert_eq!(steps, [PostLinkStep::Objcopy]);
        assert!(!plan.raw_output.exists());
        assert!(!plan.legacy_output.exists());
        fs::remove_dir_all(output_dir).unwrap();
    }

    #[test]
    fn mkimage_failure_removes_both_outputs() {
        let output_dir = temp_dir("mkimage-failure");
        let uboot = uboot();
        let plan = UbootPostLink::new(
            &Arch::RiscV64,
            &uboot,
            Path::new("build/anemone.elf"),
            &output_dir,
        );
        let mut steps = Vec::new();
        let error = plan
            .execute_with(|step, _| {
                steps.push(step);
                match step {
                    PostLinkStep::Objcopy => fs::write(&plan.raw_output, b"raw").unwrap(),
                    PostLinkStep::Mkimage => {
                        fs::write(&plan.legacy_output, b"partial image").unwrap();
                        anyhow::bail!("mkimage fixture failed");
                    },
                }
                Ok(())
            })
            .unwrap_err();

        assert!(error.to_string().contains("mkimage fixture failed"));
        assert_eq!(steps, [PostLinkStep::Objcopy, PostLinkStep::Mkimage]);
        assert!(!plan.raw_output.exists());
        assert!(!plan.legacy_output.exists());
        fs::remove_dir_all(output_dir).unwrap();
    }

    #[test]
    fn missing_tool_error_names_program_and_action() {
        let mut command = Command::new("anemone-missing-mkimage-test");
        let error = run_command(PostLinkStep::Mkimage, &mut command).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("build U-Boot legacy image"));
        assert!(message.contains("anemone-missing-mkimage-test"));
    }
}
