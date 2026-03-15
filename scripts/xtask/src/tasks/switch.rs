use std::fs;

use anyhow::Ok;
use clap::Args;

use crate::{config::{KConfig, PlatformConfig}, log_progress, workspace::PLATFORM_CONFIGS_PATH};

#[derive(Args)]
pub struct SwitchArgs {
    #[arg(help = "Build name or abbreviation")]
    pub build_name: String,
}

pub fn run(args: SwitchArgs) -> anyhow::Result<()> {
    log_progress!(
        "SWITCH",
        &format!("Searching build configuration '{}'", args.build_name)
    );

    let try_path = format!("{}/{}.toml", PLATFORM_CONFIGS_PATH, args.build_name);
    let mut name: Option<_> = None;
    if fs::exists(try_path)? {
        log_progress!(
            "SWITCH",
            &format!("Found build configuration '{}'", args.build_name)
        );
        name = Some(args.build_name.clone());
    } else {
        for entry in fs::read_dir(PLATFORM_CONFIGS_PATH)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("toml") {
                let content = fs::read_to_string(&path)?;
                let config = PlatformConfig::from_str(&content)?;
                if config.build.name == args.build_name
                    || config.build.abbrs.contains(&args.build_name)
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
                &format!("Build configuration '{}' not found", args.build_name)
            );
            return Err(anyhow::anyhow!(
                "Build configuration '{}' not found",
                args.build_name
            ));
        },
        Some(name) => {
            log_progress!(
                "SWITCH",
                &format!("Switching to build configuration '{}'", args.build_name)
            );
            let platform_config_content = std::fs::read_to_string("kconfig")?;
            let mut platform_config = KConfig::from_str(&platform_config_content)?;
            platform_config.build.platform = name;
            let new_content = toml::to_string(&platform_config)?;
            std::fs::write("kconfig", new_content)?;
            log_progress!(
                "SWITCH",
                &format!("Switched to build configuration '{}'", args.build_name)
            );
            Ok(())
        },
    }
}

pub fn list() -> anyhow::Result<()> {
    for entry in fs::read_dir(PLATFORM_CONFIGS_PATH)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("toml") {
            log_progress!(
                "CONFIG",
                &format!(
                    "{} (abbrs: {})",
                    path.file_stem().and_then(|s| s.to_str()).unwrap_or(""),
                    {
                        let content = fs::read_to_string(&path)?;
                        let config = PlatformConfig::from_str(&content)?;
                        config.build.abbrs.join(", ")
                    }
                )
            );
        }
    }
    Ok(())
}
