use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use clap::{Args, Subcommand};

use crate::{
    config::{
        reference::{BuildPresetRef, SystemTargetRef},
        resolve::ConfigLoader,
    },
    log_progress,
    workspace::{LOCAL_SELECTION_PATH, SYSTEM_TARGET_CONFIGS_PATH},
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

#[derive(Args)]
#[command(arg_required_else_help = true)]
pub struct Selection {
    #[command(subcommand)]
    command: SelectionCommands,
}

#[derive(Subcommand)]
enum SelectionCommands {
    #[command(about = "Show the effective interactive preset reference")]
    Show,
    #[command(about = "Set the developer-local interactive preset reference")]
    Set(SelectionSetArgs),
    #[command(about = "Clear the developer-local interactive preset reference")]
    Clear,
}

#[derive(Args)]
pub struct SelectionSetArgs {
    #[arg(value_name = "PRESET")]
    preset: String,
}

pub fn run_selection(args: Selection) -> anyhow::Result<()> {
    let loader = ConfigLoader::new(Path::new("."));
    match args.command {
        SelectionCommands::Show => {
            let (preset, source) = loader.implicit_preset()?;
            println!("selection source={} preset={preset}", source.as_str());
            Ok(())
        },
        SelectionCommands::Set(args) => {
            let preset = BuildPresetRef::new(&args.preset)?;
            loader.load_preset(&preset)?;
            write_selection_file(Path::new(LOCAL_SELECTION_PATH), &preset)?;
            log_progress!("SELECTION", &format!("set local preset to `{preset}`"));
            Ok(())
        },
        SelectionCommands::Clear => match fs::symlink_metadata(LOCAL_SELECTION_PATH) {
            Ok(_) => {
                fs::remove_file(LOCAL_SELECTION_PATH)?;
                log_progress!("SELECTION", "cleared local preset");
                Ok(())
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                log_progress!("SELECTION", "local preset is already clear");
                Ok(())
            },
            Err(error) => Err(error.into()),
        },
    }
}

fn write_selection_file(path: &Path, preset: &BuildPresetRef) -> anyhow::Result<()> {
    let temporary = PathBuf::from(format!("{}.tmp-{}", path.display(), std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| -> std::io::Result<()> {
        file.write_all(format!("preset = \"{preset}\"\n").as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
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

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    #[test]
    fn selection_set_replaces_target_symlink_without_following_it() {
        let workspace = TestWorkspace::new();
        let target = workspace.0.join("selection.toml");
        let outside = workspace.0.join("outside.toml");
        fs::write(&outside, "outside\n").unwrap();
        symlink(&outside, &target).unwrap();

        write_selection_file(&target, &BuildPresetRef::new("test-preset").unwrap()).unwrap();

        assert_eq!(fs::read_to_string(&outside).unwrap(), "outside\n");
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "preset = \"test-preset\"\n"
        );
        assert!(
            !fs::symlink_metadata(&target)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn selection_set_refuses_preexisting_temporary_symlink() {
        let workspace = TestWorkspace::new();
        let target = workspace.0.join("selection.toml");
        let outside = workspace.0.join("outside.toml");
        let temporary = PathBuf::from(format!("{}.tmp-{}", target.display(), std::process::id()));
        fs::write(&target, "preset = \"old\"\n").unwrap();
        fs::write(&outside, "outside\n").unwrap();
        symlink(&outside, &temporary).unwrap();

        assert!(
            write_selection_file(&target, &BuildPresetRef::new("test-preset").unwrap()).is_err()
        );
        assert_eq!(fs::read_to_string(&outside).unwrap(), "outside\n");
        assert_eq!(fs::read_to_string(&target).unwrap(), "preset = \"old\"\n");
    }

    struct TestWorkspace(PathBuf);

    impl TestWorkspace {
        fn new() -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "anemone-xtask-selection-write-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
