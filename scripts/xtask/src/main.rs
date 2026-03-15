//! Anemone build system is split into two halves:
//! The top half is the `xtask` crate, which is a custom build tool written in
//! Rust. The bottom half is Cargo and other binutils that `xtask` orchestrates.
//!
//! Xtask provides a higher-level interface for building Anemone, thus achieving
//! better usability, and customizability compared to using Cargo
//! directly.
#![allow(unused)]

use clap::{Args, Parser, Subcommand};

use crate::tasks::conf::SwitchArgs;

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
    Conf(Conf),
    #[command(about = "Build Anemone")]
    Build(tasks::build::BuildArgs),
    #[command(about = "Run Anemone in QEMU emulator")]
    Qemu(tasks::qemu::QemuArgs),
    #[command(about = "Clean build artifacts")]
    Clean,
    #[command(about = "Clean everything including config files")]
    Mrproper,
}
#[derive(Args)]
#[command(arg_required_else_help = true)]
pub struct Conf {
    #[command(subcommand)]
    command: ConfCommands,
}

#[derive(Subcommand)]
enum ConfCommands {
    #[command(about = "List all available build configurations and its abbrevations")]
    #[command(visible_alias = "ls")]
    List,
    #[command(about = "Switch to a different build configuration")]
    Switch(SwitchArgs),
}

fn main() -> anyhow::Result<()> {
    // xtask cwd: scripts/xtask
    // we need to cd to workspace root
    std::env::set_current_dir("../..")?;

    let cli = Cli::parse();
    match cli.command {
        Commands::Conf(conf) => match conf.command {
            ConfCommands::List => tasks::conf::list(),
            ConfCommands::Switch(args) => tasks::conf::switch(args),
        },
        Commands::Build(args) => tasks::build::run(args),
        Commands::Qemu(args) => tasks::qemu::run(args),
        Commands::Clean => tasks::clean::run(),
        Commands::Mrproper => tasks::mrproper::run(),
    }
}
