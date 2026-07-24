//! Anemone build system is split into two halves:
//! The top half is the `xtask` crate, which is a custom build tool written in
//! Rust. The bottom half is Cargo and other binutils that `xtask` orchestrates.
//!
//! Xtask provides a higher-level interface for building Anemone, thus achieving
//! better usability, and customizability compared to using Cargo
//! directly.

#![allow(unused)]

use clap::{Parser, Subcommand};
use std::process::ExitCode;

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
    #[command(about = "Anemone rootfs related tasks")]
    Rootfs(tasks::rootfs::RootfsArgs),
    #[command(about = "App related tasks")]
    App(tasks::app::AppArgs),
    #[command(about = "Manage curated external source references")]
    Xref(tasks::xref::XrefArgs),
    #[command(about = "Build Anemone")]
    Build(tasks::build::BuildArgs),
    #[command(about = "Format Rust sources")]
    Fmt(tasks::fmt::FmtArgs),
    #[command(about = "Run Anemone in QEMU emulator")]
    Qemu(tasks::qemu::QemuArgs),
    #[command(about = "Clean build artifacts")]
    Clean,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:#}");
            ExitCode::FAILURE
        },
    }
}

fn run() -> anyhow::Result<()> {
    // xtask cwd: scripts/xtask
    // we need to cd to workspace root
    std::env::set_current_dir("../..")?;

    let cli = Cli::parse();
    match cli.command {
        Commands::Conf(conf) => tasks::conf::run(conf),
        Commands::Rootfs(args) => tasks::rootfs::run(args),
        Commands::App(args) => tasks::app::run(args),
        Commands::Xref(args) => tasks::xref::run(args),
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
                "example",
                "--image",
                "build/anemone.elf",
            ],
            vec!["xtask", "conf", "switch", "example"],
            vec!["xtask", "mrproper"],
            vec!["xtask", "selection", "show"],
            vec!["xtask", "fmt"],
        ] {
            assert!(Cli::try_parse_from(arguments).is_err());
        }
    }

    #[test]
    fn production_cli_exposes_selection_aware_build_and_qemu() {
        assert!(
            Cli::try_parse_from(["xtask", "build", "--preset", "example", "--disasm",]).is_ok()
        );
        assert!(Cli::try_parse_from(["xtask", "fmt", "all", "--check"]).is_ok());
        assert!(Cli::try_parse_from(["xtask", "xref", "list"]).is_ok());
        assert!(Cli::try_parse_from(["xtask", "xref", "fetch", "linux-6.6.32"]).is_ok());
        assert!(
            Cli::try_parse_from([
                "xtask",
                "xref",
                "fetch",
                "linux-6.6.32",
                "--root",
                "elsewhere",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "xtask",
                "qemu",
                "--target",
                "example",
                "--kernel-config",
                "conf/.defconfig",
                "--profile",
                "release",
                "--bind",
                "kernel-image=build/anemone.elf",
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from(["xtask", "qemu", "dt", "refresh", "--platform", "example",])
                .is_err()
        );
    }
}
