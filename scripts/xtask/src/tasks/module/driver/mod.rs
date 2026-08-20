use std::path::{Path, PathBuf};

use crate::config::nemophila_module::Build;

pub use cargo::CargoDriver;

mod cargo;

pub const MODULE_TARGET: &str = "wasm32v1-none";

pub struct BuildContext<'a> {
    pub identity: &'a str,
    pub build: &'a Build,
    pub workdir: &'a Path,
    pub manifest: &'a Path,
    pub target_dir: &'a Path,
    pub candidate_dir: &'a Path,
}

#[derive(Debug)]
pub struct Candidate {
    pub path: PathBuf,
    pub driver: &'static str,
}

/// Executes one module toolchain and returns only this invocation's candidate.
///
/// Candidate freshness and stable export remain owned by the common module
/// build path.
pub trait ModuleBuildDriver {
    fn build(&self, context: &BuildContext<'_>) -> anyhow::Result<Candidate>;
}
