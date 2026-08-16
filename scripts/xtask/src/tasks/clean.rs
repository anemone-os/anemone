//! Clean all build artifacts produced by `xtask build`.

use xshell::Shell;

use crate::log_progress;

const GENERATED_KERNEL_INPUTS: &[&str] = &[
    "anemone-kernel/src/kconfig_defs.rs",
    "anemone-kernel/src/platform_defs.rs",
    "anemone-kernel/src/system_target_defs.rs",
];

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
        .args(GENERATED_KERNEL_INPUTS)
        .arg("anemone-kernel/src/arch/riscv64/generated.dtb")
        .arg("anemone-kernel/src/arch/loongarch64/generated.dtb")
        .run_echo()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_removes_every_generated_kernel_input() {
        assert!(GENERATED_KERNEL_INPUTS.contains(&"anemone-kernel/src/kconfig_defs.rs"));
        assert!(GENERATED_KERNEL_INPUTS.contains(&"anemone-kernel/src/platform_defs.rs"));
        assert!(GENERATED_KERNEL_INPUTS.contains(&"anemone-kernel/src/system_target_defs.rs"));
        assert_eq!(GENERATED_KERNEL_INPUTS.len(), 3);
    }
}
