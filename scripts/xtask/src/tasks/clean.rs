//! Clean all build artifacts produced by `xtask build`.

use xshell::Shell;

use crate::log_progress;

pub fn run() -> anyhow::Result<()> {
    log_progress!("CLEAN", "Cleaning build artifacts");
    let sh = Shell::new()?;
    sh.cmd("rm").arg("-rf").arg("build").run_echo()?;
    sh.cmd("cargo").arg("clean").run_echo()?;
    Ok(())
}
