use std::{fs, path::Path};

use clap::{Args, Subcommand};

use crate::{
    config::{reference::SystemTargetRef, resolve::ConfigLoader},
    log_progress,
    workspace::SYSTEM_TARGET_CONFIGS_PATH,
};

#[derive(Args)]
#[command(arg_required_else_help = true)]
pub struct Conf {
    #[command(subcommand)]
    command: ConfCommands,
}

#[derive(Subcommand)]
pub enum ConfCommands {
    #[command(about = "List canonical system targets and their Platforms")]
    List,
}

pub fn run(args: Conf) -> anyhow::Result<()> {
    match args.command {
        ConfCommands::List => list(),
    }
}

fn list() -> anyhow::Result<()> {
    let loader = ConfigLoader::new(Path::new("."));
    let mut paths = fs::read_dir(SYSTEM_TARGET_CONFIGS_PATH)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    for path in paths {
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("toml") {
            let target_ref =
                SystemTargetRef::new(path.file_stem().and_then(|stem| stem.to_str()).ok_or_else(
                    || anyhow::anyhow!("system target filename is not valid UTF-8"),
                )?)?;
            let target = loader.load_target(&target_ref)?;
            loader.load_platform(&target.platform)?;
            log_progress!(
                "CONFIG",
                &format!("target={target_ref} platform={}", target.platform)
            );
        }
    }
    Ok(())
}
