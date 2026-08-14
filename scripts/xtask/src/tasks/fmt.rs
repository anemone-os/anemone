use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, bail};
use clap::Args;

use crate::{log_progress, tasks::utils::cmd_echo};

const APPS_DIR: &str = "anemone-apps";
const MODULES_DIR: &str = "nemophila/modules";
const NEMOPHILA_SDK_MANIFEST: &str = "nemophila/sdk/rust/Cargo.toml";
const RUSTFMT_CONFIG: &str = "rustfmt.toml";

#[derive(Args, Debug)]
pub struct FmtArgs {
    #[arg(value_name = "SCOPE")]
    #[arg(help = "Explicit scope: `all`, `kernel`, `modules`, or an app/module name")]
    pub scope: String,

    #[arg(long, help = "Run rustfmt in check mode without writing changes")]
    pub check: bool,
}

pub fn run(args: FmtArgs) -> anyhow::Result<()> {
    let config_path = Path::new(RUSTFMT_CONFIG)
        .canonicalize()
        .with_context(|| format!("failed to locate repository {}", RUSTFMT_CONFIG))?;

    match args.scope.as_str() {
        "all" => {
            fmt_kernel_workspace(&config_path, args.check)?;
            fmt_xtask(&config_path, args.check)?;
            fmt_nemophila_sdk(&config_path, args.check)?;
            for module in module_names()? {
                fmt_module(&module, &config_path, args.check)?;
            }
            for app in app_names()? {
                fmt_app(&app, &config_path, args.check)?;
            }
            Ok(())
        },
        "kernel" => fmt_kernel_workspace(&config_path, args.check),
        "modules" => {
            fmt_nemophila_sdk(&config_path, args.check)?;
            for module in module_names()? {
                fmt_module(&module, &config_path, args.check)?;
            }
            Ok(())
        },
        scope if scope.starts_with("app:") => {
            let app = &scope["app:".len()..];
            if !app_manifest_path(app).exists() {
                bail!("unknown app format scope `{app}`")
            }
            fmt_app(app, &config_path, args.check)
        },
        scope if scope.starts_with("module:") => {
            let module = &scope["module:".len()..];
            if !module_manifest_path(module).exists() {
                bail!("unknown module format scope `{module}`")
            }
            fmt_module(module, &config_path, args.check)
        },
        package => match (
            app_manifest_path(package).exists(),
            module_manifest_path(package).exists(),
        ) {
            (true, false) => fmt_app(package, &config_path, args.check),
            (false, true) => fmt_module(package, &config_path, args.check),
            (true, true) => bail!(
                "ambiguous format scope `{package}`; use `app:{package}` or `module:{package}`"
            ),
            (false, false) => bail!(
                "unknown format scope `{package}`; expected `all`, `kernel`, `modules`, or an app/module name"
            ),
        },
    }
}

fn fmt_kernel_workspace(config_path: &Path, check: bool) -> anyhow::Result<()> {
    log_progress!("FMT", "Formatting kernel workspace");

    let mut cmd = base_cargo_fmt_cmd(check);
    cmd.arg("--manifest-path").arg("Cargo.toml").arg("--all");
    add_rustfmt_config(&mut cmd, config_path);
    run_cmd(cmd, "kernel workspace")
}

fn fmt_xtask(config_path: &Path, check: bool) -> anyhow::Result<()> {
    log_progress!("FMT", "Formatting xtask");
    let mut cmd = base_cargo_fmt_cmd(check);
    cmd.arg("--manifest-path").arg("scripts/xtask/Cargo.toml");
    add_rustfmt_config(&mut cmd, config_path);
    run_cmd(cmd, "xtask")
}

fn fmt_nemophila_sdk(config_path: &Path, check: bool) -> anyhow::Result<()> {
    log_progress!("FMT", "Formatting Nemophila Rust SDK");
    fmt_manifest(
        Path::new(NEMOPHILA_SDK_MANIFEST),
        "Nemophila Rust SDK",
        config_path,
        check,
    )
}

fn fmt_module(module: &str, config_path: &Path, check: bool) -> anyhow::Result<()> {
    log_progress!("FMT", &format!("Formatting Nemophila module '{}'", module));
    fmt_manifest(
        &module_manifest_path(module),
        &format!("Nemophila module '{}'", module),
        config_path,
        check,
    )?;

    let host_fixture = module_host_fixture_manifest_path(module);
    if host_fixture.exists() {
        fmt_manifest(
            &host_fixture,
            &format!("Nemophila module '{}' host fixture", module),
            config_path,
            check,
        )?;
    }
    Ok(())
}

fn fmt_manifest(
    manifest_path: &Path,
    target: &str,
    config_path: &Path,
    check: bool,
) -> anyhow::Result<()> {
    let mut cmd = base_cargo_fmt_cmd(check);
    cmd.arg("--manifest-path").arg(manifest_path);
    add_rustfmt_config(&mut cmd, config_path);
    run_cmd(cmd, target)
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
    package_names(Path::new(APPS_DIR))
}

fn module_names() -> anyhow::Result<Vec<String>> {
    package_names(Path::new(MODULES_DIR))
}

fn package_names(root: &Path) -> anyhow::Result<Vec<String>> {
    let mut names = Vec::new();

    for entry in fs::read_dir(root).with_context(|| format!("failed to read {}", root.display()))? {
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

fn module_manifest_path(module: &str) -> PathBuf {
    Path::new(MODULES_DIR).join(module).join("Cargo.toml")
}

fn module_host_fixture_manifest_path(module: &str) -> PathBuf {
    Path::new(MODULES_DIR)
        .join(module)
        .join("host-fixture")
        .join("Cargo.toml")
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
