use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{Context, ensure};

use crate::{
    config::nemophila_module::{
        ModuleBuildDriver as DriverKind, NemophilaModule, validate_identity,
    },
    log_progress,
};

use super::driver::{BuildContext, Candidate, CargoDriver, MODULE_TARGET, ModuleBuildDriver};

const MODULES_DIR: &str = "nemophila/modules";
const BUILD_DIR: &str = "build/modules";
static NEXT_INVOCATION: AtomicU64 = AtomicU64::new(0);

pub fn run(identity: &str) -> anyhow::Result<()> {
    build(identity).map(|_| ())
}

pub(crate) struct ModuleExport {
    pub(crate) path: PathBuf,
    /// Immutable bytes published by this invocation. Consumers must use this
    /// handoff instead of reopening `path`, which another module build may
    /// atomically replace after this function returns.
    pub(crate) bytes: Box<[u8]>,
}

pub(crate) fn build(identity: &str) -> anyhow::Result<ModuleExport> {
    validate_identity(identity)?;
    let repository = std::env::current_dir()?.canonicalize()?;
    let module_dir = repository.join(MODULES_DIR).join(identity);
    let manifest_path = module_dir.join("module.toml");
    let content = fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "failed to read module '{}' manifest '{}'",
            identity,
            manifest_path.display()
        )
    })?;
    let manifest = NemophilaModule::from_str(&content)
        .with_context(|| format!("module '{identity}' manifest is invalid"))?;
    ensure!(
        manifest.name == identity,
        "module identity '{identity}' does not match manifest name '{}'",
        manifest.name
    );

    let module_dir = canonical_directory(&module_dir, "module directory")?;
    let workdir = canonical_directory(&module_dir.join(&manifest.build.workdir), "build.workdir")?;
    ensure_contained(&module_dir, &workdir, "build.workdir")?;
    let build_manifest = canonical_file(&workdir.join(&manifest.build.manifest), "build.manifest")?;
    ensure_contained(&module_dir, &build_manifest, "build.manifest")?;

    let build_root = repository.join(BUILD_DIR);
    fs::create_dir_all(build_root.join(".candidates"))?;
    let target_dir = fresh_target_dir(&build_root.join(".candidates"), identity)?;
    let export = build_root.join(identity).join(&manifest.build.artifact);
    remove_stable_export(&export)?;

    log_progress!(
        "MODULE",
        &format!(
            "Building '{}' with cargo for {} into {}",
            identity,
            MODULE_TARGET,
            target_dir.display()
        )
    );
    let context = BuildContext {
        identity,
        build: &manifest.build,
        workdir: &workdir,
        manifest: &build_manifest,
        target_dir: &target_dir,
    };
    let candidate = match manifest.build.driver {
        DriverKind::Cargo => CargoDriver::default().build(&context),
    }
    .with_context(|| format!("module '{identity}' build driver failed"))?;
    let candidate = verify_fresh_candidate(&target_dir, candidate)?;
    let bytes = fs::read(&candidate.path).with_context(|| {
        format!(
            "failed to read module candidate '{}'",
            candidate.path.display()
        )
    })?;
    atomic_export(&bytes, &export)?;

    log_progress!(
        "MODULE",
        &format!(
            "Built with driver {}, candidate {}",
            candidate.driver,
            candidate.path.display(),
        )
    );
    log_progress!("MODULE", &format!("Exported '{}'", export.display()));
    Ok(ModuleExport {
        path: export
            .strip_prefix(&repository)
            .expect("module export is repository-local")
            .to_path_buf(),
        bytes: bytes.into_boxed_slice(),
    })
}

fn canonical_directory(path: &Path, field: &str) -> anyhow::Result<PathBuf> {
    let path = path
        .canonicalize()
        .with_context(|| format!("failed to resolve {field} '{}'", path.display()))?;
    ensure!(
        path.is_dir(),
        "{field} '{}' is not a directory",
        path.display()
    );
    Ok(path)
}

fn canonical_file(path: &Path, field: &str) -> anyhow::Result<PathBuf> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect {field} '{}'", path.display()))?;
    ensure!(
        metadata.file_type().is_file(),
        "{field} '{}' is not an ordinary file",
        path.display()
    );
    path.canonicalize()
        .with_context(|| format!("failed to resolve {field} '{}'", path.display()))
}

fn ensure_contained(root: &Path, path: &Path, field: &str) -> anyhow::Result<()> {
    ensure!(
        path.starts_with(root),
        "{field} '{}' escapes module directory '{}'",
        path.display(),
        root.display()
    );
    Ok(())
}

fn fresh_target_dir(root: &Path, identity: &str) -> anyhow::Result<PathBuf> {
    loop {
        let sequence = NEXT_INVOCATION.fetch_add(1, Ordering::Relaxed);
        let path = root.join(format!("{identity}-{}-{sequence}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => return path.canonicalize().map_err(Into::into),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to create fresh candidate directory '{}'",
                        path.display()
                    )
                });
            },
        }
    }
}

fn remove_stable_export(export: &Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(export) {
        Ok(metadata) if metadata.file_type().is_dir() => fs::remove_dir_all(export)?,
        Ok(_) => fs::remove_file(export)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn verify_fresh_candidate(target_dir: &Path, candidate: Candidate) -> anyhow::Result<Candidate> {
    let metadata = fs::symlink_metadata(&candidate.path).with_context(|| {
        format!(
            "driver returned missing candidate '{}'",
            candidate.path.display()
        )
    })?;
    ensure!(
        metadata.file_type().is_file(),
        "driver candidate '{}' is not an ordinary file",
        candidate.path.display()
    );
    let path = candidate.path.canonicalize()?;
    ensure!(
        path.starts_with(target_dir),
        "driver returned stale candidate '{}' outside this invocation '{}'",
        path.display(),
        target_dir.display()
    );
    Ok(Candidate { path, ..candidate })
}

fn atomic_export(bytes: &[u8], export: &Path) -> anyhow::Result<()> {
    let parent = export.parent().expect("module export has a parent");
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        export.file_name().unwrap().to_string_lossy(),
        std::process::id()
    ));
    remove_stable_export(&temporary)?;
    let mut file = File::create(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, export)?;
    ensure!(
        fs::symlink_metadata(export)?.file_type().is_file(),
        "published module export '{}' is not an ordinary file",
        export.display()
    );
    Ok(())
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
                "anemone-module-build-{label}-{}-{}",
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

    #[test]
    fn common_path_rejects_stale_missing_and_nonregular_candidates() {
        let temp = TempDir::new("freshness");
        let invocation = temp.0.join("invocation");
        fs::create_dir(&invocation).unwrap();
        let stale = temp.0.join("stale.wasm");
        File::create(&stale).unwrap();
        let candidate = Candidate {
            path: stale,
            driver: "fake",
        };
        assert!(
            verify_fresh_candidate(&invocation, candidate)
                .unwrap_err()
                .to_string()
                .contains("stale candidate")
        );

        let missing = Candidate {
            path: invocation.join("missing.wasm"),
            driver: "fake",
        };
        assert!(
            verify_fresh_candidate(&invocation, missing)
                .unwrap_err()
                .to_string()
                .contains("missing candidate")
        );

        let directory = invocation.join("directory.wasm");
        fs::create_dir(&directory).unwrap();
        let candidate = Candidate {
            path: directory,
            driver: "fake",
        };
        assert!(
            verify_fresh_candidate(&invocation, candidate)
                .unwrap_err()
                .to_string()
                .contains("ordinary file")
        );
    }

    #[test]
    fn common_export_is_byte_transparent_and_replaces_with_an_ordinary_file() {
        let temp = TempDir::new("export");
        let export = temp.0.join("stable/module.wasm");
        fs::create_dir_all(export.parent().unwrap()).unwrap();
        fs::write(&export, b"stale").unwrap();
        // Build/export deliberately does not interpret module bytes. Runtime
        // admission or an owner-local fixture may reject this payload later.
        atomic_export(b"not validated by module build", &export).unwrap();
        assert_eq!(fs::read(&export).unwrap(), b"not validated by module build");
        assert!(fs::symlink_metadata(&export).unwrap().file_type().is_file());
    }
}
