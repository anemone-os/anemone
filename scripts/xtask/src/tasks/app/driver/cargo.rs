use std::process::Command;

use anyhow::Context;

use crate::{config::app::CargoBuild, tasks::app::driver::DriverContext};

pub fn build_command(
    build: &CargoBuild,
    ctx: &DriverContext<'_>,
    extra_args: &[String],
) -> anyhow::Result<Command> {
    let (program, args) = build
        .args
        .split_first()
        .context("cargo driver args must not be empty")?;

    let mut cmd = Command::new("cargo");
    cmd.arg(program);
    cmd.args(args);
    cmd.arg("-Z");
    cmd.arg("build-std=core,alloc");
    cmd.arg("-Z");
    cmd.arg("build-std-features=compiler-builtins-mem");
    cmd.arg("-Z");
    cmd.arg("json-target-spec");
    cmd.arg("--target");

    // note that we are now in app's workdir, but the target spec path is relative
    // to workspace root, so we need to canonicalize it first and then pass the
    // absolute path to cargo.
    let rel_path = ctx.context.target_triple().spec_json_path().to_path_buf();
    let abs_path = rel_path
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize target spec path: {:?}", rel_path))?;

    cmd.arg(&abs_path);
    cmd.args(extra_args);
    cmd.current_dir(ctx.workdir);

    Ok(cmd)
}
