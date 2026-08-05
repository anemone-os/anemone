use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, bail};
use clap::Args;

use super::driver::{self, DriverContext};

use crate::{
    config::{
        app::{App, AppTarget, Artifact},
        build::RUST_OBJDUMP,
        platform::{Arch, TargetTriple},
    },
    log_progress,
    tasks::utils::cmd_echo,
};

#[derive(Args, Debug)]
pub struct BuildArgs {
    #[arg(help = "Name of the app to build")]
    pub app: String,

    #[arg(
        long,
        help = "App build target: riscv64, loongarch64, or host"
    )]
    pub arch: String,

    #[arg(
        short,
        long,
        help = "Whether to disassemble the built artifact for debugging"
    )]
    pub disasm: bool,

    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        help = "Extra driver arguments appended after Cargo args or Command argv; Source rejects them"
    )]
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
    target: AppTarget,
}

impl BuildCtx {
    pub fn new(arch: Arch) -> anyhow::Result<Self> {
        Ok(Self::for_target(arch.into()))
    }

    pub fn for_target(target: AppTarget) -> Self {
        Self { target }
    }

    pub fn target_triple(&self) -> Option<TargetTriple> {
        self.target.target_triple()
    }

    pub fn target_name(&self) -> &str {
        self.target.as_str()
    }

    pub fn target(&self) -> &AppTarget {
        &self.target
    }
}

pub fn run(args: BuildArgs) -> anyhow::Result<()> {
    let context = BuildCtx::for_target(AppTarget::try_from_str(&args.arch)?);

    build_app(&args.app, &args.args, &context, args.disasm)?;
    Ok(())
}

pub fn build_app(
    name: &str,
    extra_args: &[String],
    context: &BuildCtx,
    disasm: bool,
) -> anyhow::Result<Vec<BuiltArtifactInfo>> {
    let manifest_path = Path::new("anemone-apps").join(name).join("app.toml");
    let content = fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "failed to read app manifest at '{}'",
            manifest_path.display()
        )
    })?;
    let app = App::from_str(&content)?;
    validate_app_reference(name, &app, &manifest_path)?;
    validate_app_target(&app, context.target(), &manifest_path)?;

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
    if let Some(mut cmd) = driver::build_command(&driver_ctx, extra_args)? {
        cmd_echo(&cmd);
        execute_build_command(&app.name, &mut cmd)?;
    }

    let mut built = Vec::with_capacity(app.artifacts.len());
    for artifact in &app.artifacts {
        let built_artifact = copy_artifact(&app, &workdir, artifact, &out_dir, context)?;
        if disasm {
            generate_artifact_disasm(&built_artifact, context)?;
        }
        built.push(built_artifact);
    }

    Ok(built)
}

fn execute_build_command(app_name: &str, command: &mut Command) -> anyhow::Result<()> {
    let status = command
        .status()
        .with_context(|| format!("failed to execute build command for app '{app_name}'"))?;
    if !status.success() {
        bail!("build command for app '{app_name}' exited with status {status}");
    }
    Ok(())
}

fn validate_app_reference(name: &str, app: &App, manifest_path: &Path) -> anyhow::Result<()> {
    if app.name != name {
        bail!(
            "app reference '{}' does not match manifest name '{}' at '{}'",
            name,
            app.name,
            manifest_path.display()
        );
    }
    Ok(())
}

fn validate_app_target(
    app: &App,
    target: &AppTarget,
    manifest_path: &Path,
) -> anyhow::Result<()> {
    if !app.targets.contains(target) {
        let declared = app
            .targets
            .iter()
            .map(AppTarget::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        bail!(
            "app '{}' does not support build target '{}' at '{}'; declared targets: {}",
            app.name,
            target.as_str(),
            manifest_path.display(),
            declared
        );
    }
    Ok(())
}

fn copy_artifact(
    app: &App,
    workdir: &Path,
    artifact: &Artifact,
    out_dir: &Path,
    context: &BuildCtx,
) -> anyhow::Result<BuiltArtifactInfo> {
    let source_path = workdir.join(expand_artifact_path(artifact, context)?);
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

fn generate_artifact_disasm(
    artifact: &BuiltArtifactInfo,
    context: &BuildCtx,
) -> anyhow::Result<()> {
    let artifact_name = artifact.name().unwrap_or_else(|| {
        artifact
            .output_path
            .as_os_str()
            .to_str()
            .unwrap_or("<unknown>")
    });
    log_progress!(
        "DISASM",
        &format!(
            "Generating source disassembly for app artifact '{}'",
            artifact_name
        )
    );

    let output = Command::new(RUST_OBJDUMP)
        .arg("-d")
        .arg("-S")
        .arg(&artifact.output_path)
        .output()
        .with_context(|| {
            format!(
                "failed to run objdump for app artifact '{}'",
                artifact.output_path.display()
            )
        })?;

    if !output.status.success() {
        bail!(
            "objdump for app artifact '{}' exited with status {}",
            artifact.output_path.display(),
            output.status
        );
    }

    let disasm_path = artifact_disasm_path(&artifact.output_path)?;
    fs::write(&disasm_path, output.stdout).with_context(|| {
        format!(
            "failed to write disassembly for app artifact '{}' to {}",
            artifact.output_path.display(),
            disasm_path.display()
        )
    })?;

    Ok(())
}

fn artifact_disasm_path(artifact_path: &Path) -> anyhow::Result<PathBuf> {
    let file_name = artifact_path.file_name().with_context(|| {
        format!(
            "artifact '{}' has no file name for disassembly output",
            artifact_path.display()
        )
    })?;
    let mut disasm_name = file_name.to_os_string();
    disasm_name.push(".disasm");
    Ok(artifact_path.with_file_name(disasm_name))
}

/// app.toml/artifact/path
fn expand_artifact_path(artifact: &Artifact, context: &BuildCtx) -> anyhow::Result<String> {
    let expanded = artifact.path.replace("${ARCH}", context.target_name());
    let Some(target_triple) = context.target_triple() else {
        if expanded.contains("${TARGET_TRIPLE}") {
            bail!(
                "artifact path '{}' cannot expand ${{TARGET_TRIPLE}} for host because host has no Anemone target triple",
                artifact.path
            );
        }
        return Ok(expanded);
    };
    Ok(expanded.replace("${TARGET_TRIPLE}", target_triple.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        app::{AppTarget, Build, BuildDriver, SourceBuild},
        platform::Arch,
    };
    use std::{
        os::unix::fs::{PermissionsExt, symlink},
        time::{SystemTime, UNIX_EPOCH},
    };

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "anemone-xtask-source-app-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn source_app(artifacts: Vec<Artifact>) -> App {
        App {
            name: "prebuilt".to_string(),
            targets: vec![AppTarget::Anemone(Arch::RiscV64)],
            build: Build {
                workdir: ".".to_string(),
                driver: BuildDriver::Source(SourceBuild {}),
            },
            artifacts,
        }
    }

    #[test]
    fn source_artifacts_share_expansion_and_export() {
        let root = TestDirectory::new();
        let workdir = root.0.join("work");
        let out_dir = root.0.join("out");
        fs::create_dir_all(workdir.join("riscv64/riscv64-unknown-anemone-elf")).unwrap();
        fs::create_dir_all(&out_dir).unwrap();

        let binary = workdir.join("riscv64/riscv64-unknown-anemone-elf/prebuilt");
        let script = workdir.join("script.sh");
        fs::write(&binary, b"prebuilt-binary\0bytes").unwrap();
        fs::write(&script, b"#!/bin/sh\nexit 97\n").unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o751)).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o640)).unwrap();

        let artifacts = vec![
            Artifact {
                path: "${ARCH}/${TARGET_TRIPLE}/prebuilt".to_string(),
            },
            Artifact {
                path: "script.sh".to_string(),
            },
        ];
        let app = source_app(artifacts.clone());
        let context = BuildCtx::new(Arch::RiscV64).unwrap();

        for artifact in &artifacts {
            let exported = copy_artifact(&app, &workdir, artifact, &out_dir, &context).unwrap();
            let source_bytes = fs::read(&exported.source_path).unwrap();
            let output_bytes = fs::read(&exported.output_path).unwrap();
            assert_eq!(source_bytes, output_bytes);

            let source_mode = fs::metadata(&exported.source_path)
                .unwrap()
                .permissions()
                .mode();
            let output_mode = fs::metadata(&exported.output_path)
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(source_mode, output_mode);
        }
    }

    #[test]
    fn source_artifacts_fail_before_export_when_input_is_invalid() {
        let root = TestDirectory::new();
        let workdir = root.0.join("work");
        let out_dir = root.0.join("out");
        fs::create_dir_all(workdir.join("directory")).unwrap();
        fs::create_dir_all(&out_dir).unwrap();
        let device = workdir.join("device");
        symlink("/dev/null", &device).unwrap();

        let context = BuildCtx::new(Arch::RiscV64).unwrap();
        for path in ["missing", "directory", "device"] {
            let artifact = Artifact {
                path: path.to_string(),
            };
            let app = source_app(vec![artifact.clone()]);
            let error = copy_artifact(&app, &workdir, &artifact, &out_dir, &context)
                .unwrap_err()
                .to_string();
            assert!(error.contains("prebuilt"), "{error}");
            assert!(error.contains(path), "{error}");
            assert!(!out_dir.join(path).exists());
        }
    }

    #[test]
    fn referenced_app_must_match_manifest_name() {
        let app = source_app(Vec::new());
        let error = validate_app_reference(
            "reference-name",
            &app,
            Path::new("anemone-apps/reference-name/app.toml"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("reference-name"), "{error}");
        assert!(error.contains("prebuilt"), "{error}");
    }

    #[test]
    fn app_target_must_be_declared() {
        let app = source_app(Vec::new());
        let error = validate_app_target(
            &app,
            &AppTarget::Host,
            Path::new("anemone-apps/prebuilt/app.toml"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("prebuilt"), "{error}");
        assert!(error.contains("host"), "{error}");
        assert!(error.contains("riscv64"), "{error}");
    }

    #[test]
    fn host_artifact_path_expands_without_a_target_triple() {
        let context = BuildCtx::for_target(AppTarget::Host);
        let artifact = Artifact {
            path: "out/${ARCH}/prebuilt".to_string(),
        };
        assert_eq!(
            expand_artifact_path(&artifact, &context).unwrap(),
            "out/host/prebuilt"
        );

        let artifact = Artifact {
            path: "target/${TARGET_TRIPLE}/prebuilt".to_string(),
        };
        let error = expand_artifact_path(&artifact, &context)
            .unwrap_err()
            .to_string();
        assert!(error.contains("host has no Anemone target triple"), "{error}");
    }

    #[test]
    fn build_command_launch_and_status_failures_are_reported() {
        let root = TestDirectory::new();
        let missing = root.0.join("missing-command");
        let mut command = Command::new(&missing);
        let error = execute_build_command("command-test", &mut command)
            .unwrap_err()
            .to_string();
        assert!(error.contains("command-test"), "{error}");
        assert!(error.contains("failed to execute"), "{error}");

        let mut command = Command::new("sh");
        command.args(["-c", "exit 23"]);
        let error = execute_build_command("command-test", &mut command)
            .unwrap_err()
            .to_string();
        assert!(error.contains("command-test"), "{error}");
        assert!(error.contains("23"), "{error}");
    }
}
