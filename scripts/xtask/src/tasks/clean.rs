//! Clean all build artifacts produced by `xtask build`.

use xshell::Shell;

use crate::log_progress;

pub fn run() -> anyhow::Result<()> {
    log_progress!("CLEAN", "Cleaning build artifacts");
    let sh = Shell::new()?;
    sh.cmd("rm").arg("-rf").arg("build").run_echo()?;
    sh.cmd("cargo").arg("clean").run_echo()?;
    sh.cmd("rm")
        .arg("-rf")
        .arg("scripts/xtask/target")
        .run_echo()?;
    sh.cmd("rm")
        .arg("-f")
        .arg("anemone-kernel/src/kconfig_defs.rs")
        .arg("anemone-kernel/src/platform_defs.rs")
        .arg("anemone-kernel/src/arch/riscv64/generated.dtb")
        .arg("anemone-kernel/src/arch/loongarch64/generated.dtb")
        .run_echo()?;
    Ok(())
}
