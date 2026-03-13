//! Not only clean build artifacts, but also
//! clean configuration files.

use xshell::Shell;

use crate::log_progress;

pub fn run() -> anyhow::Result<()> {
    log_progress!(
        "MrProper",
        "Cleaning build artifacts and configuration files",
    );
    let sh = Shell::new()?;
    sh.cmd("rm").arg("-rf").arg("build").run_echo()?;
    sh.cmd("cargo").arg("clean").run_echo()?;
    sh.cmd("rm")
        .arg("-f")
        .arg("anemone-kernel/src/kconfig_defs.rs")
        .arg("anemone-kernel/src/platform_defs.rs")
        .run_echo()?;
    sh.cmd("rm").arg("-f").arg("kconfig").run_echo()?;
    Ok(())
}
