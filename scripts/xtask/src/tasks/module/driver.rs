use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, bail, ensure};

use crate::{config::nemophila_module::Build, tasks::utils::cmd_echo};

pub const MODULE_TARGET: &str = "wasm32v1-none";
pub const MODULE_PROFILE: &str = "nemophila-module";

pub struct BuildContext<'a> {
    pub identity: &'a str,
    pub build: &'a Build,
    pub workdir: &'a Path,
    pub cargo_manifest: &'a Path,
    pub target_dir: &'a Path,
}

#[derive(Debug)]
pub struct Candidate {
    pub path: PathBuf,
    pub driver: &'static str,
}

/// Executes one module toolchain and returns only this invocation's candidate.
///
/// Artifact policy, validation, execution, and stable export remain owned by
/// the common module build path.
pub trait ModuleBuildDriver {
    fn build(&self, context: &BuildContext<'_>) -> anyhow::Result<Candidate>;
}

pub struct CargoDriver {
    program: OsString,
}

impl Default for CargoDriver {
    fn default() -> Self {
        Self {
            program: OsString::from("cargo"),
        }
    }
}

impl CargoDriver {
    #[cfg(test)]
    fn with_program(program: impl AsRef<OsStr>) -> Self {
        Self {
            program: program.as_ref().to_owned(),
        }
    }
}

impl ModuleBuildDriver for CargoDriver {
    fn build(&self, context: &BuildContext<'_>) -> anyhow::Result<Candidate> {
        let mut command = Command::new(&self.program);
        command
            .current_dir(context.workdir)
            .arg("build")
            .arg("--locked")
            .arg("--manifest-path")
            .arg(context.cargo_manifest)
            .arg("--package")
            .arg(&context.build.package)
            .arg("--target")
            .arg(MODULE_TARGET)
            .arg("--profile")
            .arg(MODULE_PROFILE)
            .arg("--target-dir")
            .arg(context.target_dir)
            .arg("-Z")
            .arg("build-std=core,alloc");

        cmd_echo(&command);
        let status = command.status().with_context(|| {
            format!(
                "module '{}' Cargo driver could not execute repository Rust toolchain '{}'",
                context.identity,
                self.program.to_string_lossy()
            )
        })?;
        ensure!(
            status.success(),
            "module '{}' Cargo driver failed for target {MODULE_TARGET} with status {status}",
            context.identity
        );

        let output_dir = context.target_dir.join(MODULE_TARGET).join(MODULE_PROFILE);
        let path = select_candidate(&output_dir, &context.build.artifact).with_context(|| {
            format!(
                "module '{}' Cargo driver did not produce its declared candidate",
                context.identity
            )
        })?;
        Ok(Candidate {
            path,
            driver: "cargo",
        })
    }
}

fn select_candidate(output_dir: &Path, expected_name: &str) -> anyhow::Result<PathBuf> {
    let entries = fs::read_dir(output_dir).with_context(|| {
        format!(
            "failed to read Cargo output directory '{}'",
            output_dir.display()
        )
    })?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.path().extension().and_then(OsStr::to_str) == Some("wasm") {
            candidates.push(entry.path());
        }
    }
    candidates.sort();
    ensure!(
        candidates.len() == 1,
        "expected exactly one top-level .wasm candidate in '{}', found {candidates:?}",
        output_dir.display()
    );
    let candidate = candidates.pop().expect("length checked");
    if candidate.file_name() != Some(OsStr::new(expected_name)) {
        bail!(
            "Cargo candidate '{}' does not match declared artifact '{expected_name}'",
            candidate.display()
        );
    }
    let file_type = fs::symlink_metadata(&candidate)?.file_type();
    ensure!(
        file_type.is_file(),
        "Cargo candidate '{}' is not an ordinary file",
        candidate.display()
    );
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use std::{
        fs::File,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "anemone-xtask-{label}-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn build_config() -> Build {
        Build {
            workdir: ".".into(),
            driver: crate::config::nemophila_module::ModuleBuildDriver::Cargo,
            manifest: "Cargo.toml".into(),
            package: "fixture".into(),
            artifact: "fixture.wasm".into(),
        }
    }

    #[test]
    fn missing_tool_and_cargo_failure_keep_driver_context() {
        let temp = TempDir::new("cargo-errors");
        let build = build_config();
        let cargo_manifest = temp.0.join("Cargo.toml");
        let target_dir = temp.0.join("target");
        let context = BuildContext {
            identity: "fixture",
            build: &build,
            workdir: &temp.0,
            cargo_manifest: &cargo_manifest,
            target_dir: &target_dir,
        };

        let error = CargoDriver::with_program(temp.0.join("missing-cargo"))
            .build(&context)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("could not execute repository Rust toolchain"),
            "{error}"
        );

        let error = CargoDriver::with_program("/bin/false")
            .build(&context)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Cargo driver failed for target"), "{error}");
    }

    #[test]
    fn candidate_selection_rejects_missing_multiple_wrong_and_nonregular_outputs() {
        let temp = TempDir::new("candidate-selection");
        let output = &temp.0;
        assert!(
            select_candidate(output, "fixture.wasm")
                .unwrap_err()
                .to_string()
                .contains("exactly one")
        );

        File::create(output.join("first.wasm")).unwrap();
        File::create(output.join("second.wasm")).unwrap();
        assert!(
            select_candidate(output, "fixture.wasm")
                .unwrap_err()
                .to_string()
                .contains("exactly one")
        );

        fs::remove_file(output.join("second.wasm")).unwrap();
        assert!(
            select_candidate(output, "fixture.wasm")
                .unwrap_err()
                .to_string()
                .contains("declared artifact")
        );

        fs::remove_file(output.join("first.wasm")).unwrap();
        fs::create_dir(output.join("fixture.wasm")).unwrap();
        assert!(
            select_candidate(output, "fixture.wasm")
                .unwrap_err()
                .to_string()
                .contains("ordinary file")
        );
    }
}
