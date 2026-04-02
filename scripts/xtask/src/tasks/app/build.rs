use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use clap::Args;

use super::driver::{self, DriverContext};

use crate::{
    config::{
        app::{App, Artifact},
        platform::{Arch, TargetTriple},
    },
    log_progress,
    tasks::utils::cmd_echo,
};

#[derive(Args, Debug)]
pub struct BuildArgs {
    #[arg(long, help = "Target architecture for the app build")]
    pub arch: String,

    pub app: String,

    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BuiltArtifactInfo {
    pub source_path: PathBuf,
    pub output_path: PathBuf,
}

impl BuiltArtifactInfo {
    pub fn name(&self) -> Option<&str> {
        self.output_path.file_name().and_then(|n| n.to_str())
    }
}

#[derive(Debug, Clone)]
pub struct BuildCtx {
    arch: Arch,
}

impl BuildCtx {
    pub fn new(arch: Arch) -> anyhow::Result<Self> {
        Ok(Self { arch })
    }

    pub fn target_triple(&self) -> TargetTriple {
        self.arch.target_triple()
    }
}

pub fn run(args: BuildArgs) -> anyhow::Result<()> {
    let context = BuildCtx::new(Arch::try_from_str(&args.arch)?)?;

    build_app(&args.app, &args.args, &context)?;
    Ok(())
}

pub fn build_app(
    name: &str,
    extra_args: &[String],
    context: &BuildCtx,
) -> anyhow::Result<Vec<BuiltArtifactInfo>> {
    let manifest_path = Path::new("anemone-apps").join(name).join("app.toml");
    let content = fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "failed to read app manifest at '{}'",
            manifest_path.display()
        )
    })?;
    let app = App::from_str(&content)?;

    let app_dir = manifest_path
        .parent()
        .context("app manifest path has no parent directory")?;
    let workdir = app_dir.join(&app.build.workdir);
    let out_dir = Path::new("build/apps").join(&app.name);

    fs::create_dir_all(&out_dir)?;

    log_progress!("APP", &format!("Building app '{}'", app.name));

    let driver_ctx = DriverContext {
        app: &app,
        workdir: &workdir,
        context,
    };
    let mut cmd = driver::build_command(&driver_ctx, extra_args)?;
    cmd_echo(&cmd);
    let status = cmd
        .status()
        .with_context(|| format!("failed to execute build command for app '{}'", app.name))?;
    if !status.success() {
        bail!(
            "build command for app '{}' exited with status {}",
            app.name,
            status
        );
    }

    let mut built = Vec::with_capacity(app.artifacts.len());
    for artifact in &app.artifacts {
        built.push(copy_artifact(&app, &workdir, artifact, &out_dir, context)?);
    }

    Ok(built)
}

fn copy_artifact(
    app: &App,
    workdir: &Path,
    artifact: &Artifact,
    out_dir: &Path,
    context: &BuildCtx,
) -> anyhow::Result<BuiltArtifactInfo> {
    let source_path = workdir.join(expand_artifact_path(artifact, context));
    if !source_path.exists() {
        bail!(
            "artifact '{}' for app '{}' does not exist after build",
            source_path.display(),
            app.name
        );
    }
    if !source_path.is_file() {
        bail!(
            "artifact '{}' for app '{}' is not a file",
            source_path.display(),
            app.name
        );
    }

    let file_name = source_path.file_name().with_context(|| {
        format!(
            "artifact '{}' for app '{}' has no file name",
            source_path.display(),
            app.name
        )
    })?;
    let output_path = out_dir.join(file_name);
    fs::copy(&source_path, &output_path).with_context(|| {
        format!(
            "failed to export app '{}' artifact '{}' to {}",
            app.name,
            source_path.display(),
            output_path.display()
        )
    })?;

    Ok(BuiltArtifactInfo {
        source_path,
        output_path,
    })
}

/// app.toml/artifact/path
fn expand_artifact_path(artifact: &Artifact, context: &BuildCtx) -> String {
    artifact
        .path
        .replace("${ARCH}", context.arch.as_str())
        .replace("${TARGET_TRIPLE}", context.target_triple().as_str())
}
