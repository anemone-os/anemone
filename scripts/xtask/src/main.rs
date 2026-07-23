//! Anemone build system is split into two halves:
//! The top half is the `xtask` crate, which is a custom build tool written in
//! Rust. The bottom half is Cargo and other binutils that `xtask` orchestrates.
//!
//! Xtask provides a higher-level interface for building Anemone, thus achieving
//! better usability, and customizability compared to using Cargo
//! directly.

#![allow(unused)]

use clap::{Parser, Subcommand};

mod config;
mod tasks;
mod workspace;

#[derive(Parser)]
#[command(name = "xtask")]
#[command(about = "Anemone build system", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Manage Anemone build configurations")]
    Conf(tasks::conf::Conf),
    #[command(about = "Manage the developer-local interactive selection")]
    Selection(tasks::conf::Selection),
    #[command(about = "Anemone rootfs related tasks")]
    Rootfs(tasks::rootfs::RootfsArgs),
    #[command(about = "App related tasks")]
    App(tasks::app::AppArgs),
    #[command(about = "Build Anemone")]
    Build(tasks::build::BuildArgs),
    #[command(about = "Format Rust sources")]
    Fmt(tasks::fmt::FmtArgs),
    #[command(about = "Run Anemone in QEMU emulator")]
    Qemu(tasks::qemu::QemuArgs),
    #[command(about = "Clean build artifacts")]
    Clean,
}

fn main() -> anyhow::Result<()> {
    // xtask cwd: scripts/xtask
    // we need to cd to workspace root
    std::env::set_current_dir("../..")?;

    let cli = Cli::parse();
    match cli.command {
        Commands::Conf(conf) => tasks::conf::run(conf),
        Commands::Selection(selection) => tasks::conf::run_selection(selection),
        Commands::Rootfs(args) => tasks::rootfs::run(args),
        Commands::App(args) => tasks::app::run(args),
        Commands::Build(args) => tasks::build::run(args),
        Commands::Fmt(args) => tasks::fmt::run(args),
        Commands::Qemu(args) => tasks::qemu::run(args),
        Commands::Clean => tasks::clean::run(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_cli_rejects_legacy_commands_and_options() {
        for arguments in [
            vec!["xtask", "build", "-k", "kconfig"],
            vec![
                "xtask",
                "qemu",
                "--platform",
                "qemu-virt-rv64",
                "--image",
                "build/anemone.elf",
            ],
            vec!["xtask", "conf", "switch", "qemu-virt-rv64"],
            vec!["xtask", "mrproper"],
        ] {
            assert!(Cli::try_parse_from(arguments).is_err());
        }
    }

    #[test]
    fn production_cli_exposes_selection_aware_build_and_qemu() {
        assert!(
            Cli::try_parse_from([
                "xtask",
                "build",
                "--preset",
                "qemu-virt-rv64-pretest-release",
                "--disasm",
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "xtask",
                "qemu",
                "--target",
                "qemu-virt-rv64-pretest",
                "--kernel-config",
                "conf/.defconfig",
                "--profile",
                "release",
                "--bind",
                "kernel-image=build/anemone.elf",
            ])
            .is_ok()
        );
    }
}
