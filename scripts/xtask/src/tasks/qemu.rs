//! Run the built OS image in QEMU emulator.
use crate::{
    config::PlatformConfig,
    tasks::utils::{cmd_echo, log_progress},
    workspace::*,
};
use clap::Args;
use xshell::Shell;

#[derive(Args)]
pub struct QemuArgs {
    #[arg(short, long)]
    #[arg(help = "Which platform to emulate")]
    platform: String,
    #[arg(short, long)]
    #[arg(help = "Path to the kernel image to run")]
    image: String,
}

pub fn run(args: QemuArgs) -> anyhow::Result<()> {
    let config_path = format!("{}/{}.toml", PLATFORM_CONFIGS_PATH, args.platform);
    let config_content = std::fs::read_to_string(config_path)?;
    let config = PlatformConfig::from_str(&config_content)?;
    if let Some(qemu) = &config.qemu {
        log_progress("QEMU", "Launching QEMU emulator...");

        // TODO: elegent echo

        let mut cmd = std::process::Command::new(&qemu.qemu);
        cmd.arg("-machine")
            .arg(&qemu.machine)
            .arg("-cpu")
            .arg(&qemu.cpu)
            .arg("-smp")
            .arg(qemu.smp.to_string())
            .arg("-m")
            .arg(&qemu.memory)
            .arg("-bios")
            .arg(&qemu.bios)
            .arg("-kernel")
            .arg(&args.image)
            .args(
                qemu.args
                    .as_ref()
                    .map(|args| args.as_slice())
                    .unwrap_or(&[]),
            );
        cmd_echo(&cmd);
        cmd.status()?;
    } else {
        anyhow::bail!(
            "QEMU configuration not found for platform {}",
            args.platform
        );
    }

    Ok(())
}
