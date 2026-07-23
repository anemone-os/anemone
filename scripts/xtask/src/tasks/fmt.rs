use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, bail};
use clap::Args;

use crate::{log_progress, tasks::utils::cmd_echo};

const APPS_DIR: &str = "anemone-apps";
const RUSTFMT_CONFIG: &str = "rustfmt.toml";

#[derive(Args, Debug)]
pub struct FmtArgs {
    #[arg(help = "Package group or app to format. Omit to format kernel and all apps.")]
    pub package: Option<String>,

    #[arg(long, help = "Run rustfmt in check mode without writing changes")]
    pub check: bool,
}

pub fn run(args: FmtArgs) -> anyhow::Result<()> {
    let config_path = Path::new(RUSTFMT_CONFIG)
        .canonicalize()
        .with_context(|| format!("failed to locate repository {}", RUSTFMT_CONFIG))?;

    match args.package.as_deref() {
        None | Some("all") => {
            fmt_kernel_workspace(&config_path, args.check)?;
            for app in app_names()? {
                fmt_app(&app, &config_path, args.check)?;
            }
            Ok(())
        },
        Some("kernel") => fmt_kernel_workspace(&config_path, args.check),
        Some(package) if app_manifest_path(package).exists() => {
            fmt_app(package, &config_path, args.check)
        },
        Some(package) => fmt_kernel_package(package, &config_path, args.check),
    }
}

fn fmt_kernel_workspace(config_path: &Path, check: bool) -> anyhow::Result<()> {
    log_progress!("FMT", "Formatting kernel workspace");

    let mut cmd = base_cargo_fmt_cmd(check);
    cmd.arg("--manifest-path").arg("Cargo.toml").arg("--all");
    add_rustfmt_config(&mut cmd, config_path);
    run_cmd(cmd, "kernel workspace")
}

fn fmt_kernel_package(package: &str, config_path: &Path, check: bool) -> anyhow::Result<()> {
    log_progress!("FMT", &format!("Formatting package '{}'", package));

    let mut cmd = base_cargo_fmt_cmd(check);
    cmd.arg("--manifest-path")
        .arg("Cargo.toml")
        .arg("--package")
        .arg(package);
    add_rustfmt_config(&mut cmd, config_path);
    run_cmd(cmd, &format!("package '{}'", package))
}

fn fmt_app(app: &str, config_path: &Path, check: bool) -> anyhow::Result<()> {
    log_progress!("FMT", &format!("Formatting app '{}'", app));

    let manifest_path = app_manifest_path(app);
    let mut cmd = base_cargo_fmt_cmd(check);
    cmd.arg("--manifest-path").arg(&manifest_path);
    add_rustfmt_config(&mut cmd, config_path);
    run_cmd(cmd, &format!("app '{}'", app))
}

fn app_names() -> anyhow::Result<Vec<String>> {
    let mut names = Vec::new();

    for entry in fs::read_dir(APPS_DIR).with_context(|| format!("failed to read {}", APPS_DIR))? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let path = entry.path();
        if !path.join("Cargo.toml").exists() {
            continue;
        }

        let name = entry
            .file_name()
            .into_string()
            .map_err(|name| anyhow::anyhow!("app directory name is not valid UTF-8: {:?}", name))?;
        names.push(name);
    }

    names.sort();
    Ok(names)
}

fn app_manifest_path(app: &str) -> PathBuf {
    Path::new(APPS_DIR).join(app).join("Cargo.toml")
}

fn base_cargo_fmt_cmd(check: bool) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg("fmt");
    if check {
        cmd.arg("--check");
    }
    cmd
}

fn add_rustfmt_config(cmd: &mut Command, config_path: &Path) {
    cmd.arg("--").arg("--config-path").arg(config_path);
}

fn run_cmd(mut cmd: Command, target: &str) -> anyhow::Result<()> {
    cmd_echo(&cmd);
    let status = cmd
        .status()
        .with_context(|| format!("failed to execute cargo fmt for {}", target))?;
    if !status.success() {
        bail!("cargo fmt for {} exited with status {}", target, status);
    }
    Ok(())
}
