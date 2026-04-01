//! Run the built OS image in QEMU emulator.
use crate::{
    config::{PlatformConfig, platform::Qemu},
    tasks::utils::{cmd_echo, log_progress},
    workspace::*,
};
use anyhow::Ok;
use clap::Args;

#[derive(Args)]
pub struct QemuArgs {
    #[arg(short, long)]
    #[arg(help = "Which platform to emulate")]
    platform: String,
    #[arg(short, long)]
    #[arg(help = "Path to the kernel image to run")]
    image: String,
}

pub fn gen_qemu_cmd(qemu: &Qemu, args: Option<&QemuArgs>) -> std::process::Command {
    let mut cmd = std::process::Command::new(&qemu.qemu);
    cmd.arg("-machine")
        .arg(&qemu.machine)
        .arg("-smp")
        .arg(qemu.smp.to_string())
        .arg("-m")
        .arg(&qemu.memory)
        .args(
            qemu.args
                .as_ref()
                .map(|args| args.as_slice())
                .unwrap_or(&[]),
        );
    if let Some(cpu) = &qemu.cpu {
        cmd.arg("-cpu").arg(cpu);
    }
    if let Some(args) = args {
        cmd.arg("-kernel").arg(args.image.clone());
    }
    if let Some(bios) = &qemu.bios {
        cmd.arg("-bios").arg(bios);
    }
    cmd
}

pub fn run(args: QemuArgs) -> anyhow::Result<()> {
    let config_path = format!("{}/{}.toml", PLATFORM_CONFIGS_PATH, args.platform);
    let config_content = std::fs::read_to_string(config_path)?;
    let config = PlatformConfig::from_str(&config_content)?;
    if let Some(qemu) = &config.qemu {
        log_progress("QEMU", "Launching QEMU emulator...");
        let mut cmd = gen_qemu_cmd(qemu, Some(&args));
        cmd_echo(&cmd);
        match cmd.status() {
            Result::Ok(status) => {
                if !status.success() {
                    anyhow::bail!("QEMU exited with status: {}", status);
                }
            },
            Err(e) => {
                anyhow::bail!("Failed to launch QEMU: {}", e);
            },
        }
    } else {
        anyhow::bail!(
            "QEMU configuration not found for platform {}",
            args.platform
        );
    }

    Ok(())
}
