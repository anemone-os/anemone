use std::{fs, str::FromStr};

use anyhow::Ok;
use clap::{Args, Subcommand};

use crate::{
    config::{PlatformConfig, system_target::Config as SystemTargetConfig},
    log_progress,
    workspace::{PLATFORM_CONFIGS_PATH, SYSTEM_TARGET_CONFIGS_PATH},
};

#[derive(Args)]
#[command(arg_required_else_help = true)]
pub struct Conf {
    #[command(subcommand)]
    command: ConfCommands,
}

#[derive(Subcommand)]
pub enum ConfCommands {
    #[command(about = "List all available build configurations and its abbrevations")]
    #[command(visible_alias = "ls")]
    List,
    #[command(about = "Switch to a different build configuration")]
    Switch(SwitchArgs),
}

pub fn run(args: Conf) -> anyhow::Result<()> {
    match args.command {
        ConfCommands::List => list(),
        ConfCommands::Switch(args) => switch(args),
    }
}

#[derive(Args)]
pub struct SwitchArgs {
    #[arg(help = "System target or legacy platform abbreviation")]
    pub target_name: String,
}

fn switch(args: SwitchArgs) -> anyhow::Result<()> {
    log_progress!(
        "SWITCH",
        &format!("Searching system target '{}'", args.target_name)
    );

    let try_path = format!("{}/{}.toml", SYSTEM_TARGET_CONFIGS_PATH, args.target_name);
    let mut name: Option<_> = None;
    if fs::exists(try_path)? {
        log_progress!(
            "SWITCH",
            &format!("Found system target '{}'", args.target_name)
        );
        name = Some(args.target_name.clone());
    } else {
        for entry in fs::read_dir(PLATFORM_CONFIGS_PATH)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("toml") {
                let content = fs::read_to_string(&path)?;
                let config = PlatformConfig::from_str(&content)?;
                if (config.build.name == args.target_name
                    || config.build.abbrs.contains(&args.target_name))
                    && fs::exists(format!(
                        "{}/{}.toml",
                        SYSTEM_TARGET_CONFIGS_PATH, config.build.name
                    ))?
                {
                    name = Some(config.build.name.clone());
                }
            }
        }
    }
    match name {
        None => {
            log_progress!(
                "ERROR",
                &format!("System target '{}' not found", args.target_name)
            );
            return Err(anyhow::anyhow!(
                "System target '{}' not found",
                args.target_name
            ));
        },
        Some(name) => {
            log_progress!(
                "SWITCH",
                &format!("Switching to system target '{}'", args.target_name)
            );
            let platform_config_content = std::fs::read_to_string("kconfig")?;
            // Stage 2 removes this bridge with the legacy kconfig selection.
            let mut doc = toml_edit::DocumentMut::from_str(&platform_config_content)?;
            doc["build"]["target"] = toml_edit::value(name);
            std::fs::write("kconfig", doc.to_string())?;
            log_progress!(
                "SWITCH",
                &format!("Switched to system target '{}'", args.target_name)
            );
            Ok(())
        },
    }
}

fn list() -> anyhow::Result<()> {
    for entry in fs::read_dir(SYSTEM_TARGET_CONFIGS_PATH)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("toml") {
            log_progress!(
                "CONFIG",
                &format!(
                    "{} (abbrs: {})",
                    path.file_stem().and_then(|s| s.to_str()).unwrap_or(""),
                    {
                        let target_content = fs::read_to_string(&path)?;
                        let target = SystemTargetConfig::from_str(&target_content)?;
                        let platform_content = fs::read_to_string(format!(
                            "{}/{}.toml",
                            PLATFORM_CONFIGS_PATH, target.platform
                        ))?;
                        let platform = PlatformConfig::from_str(&platform_content)?;
                        platform.build.abbrs.join(", ")
                    }
                )
            );
        }
    }
    Ok(())
}
