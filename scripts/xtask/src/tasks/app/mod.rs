//! App related tasks.

use clap::{Args, Subcommand};

mod driver;

pub mod build;

#[derive(Args)]
pub struct AppArgs {
    #[command(subcommand)]
    command: AppCommand,
}

#[derive(Subcommand)]
pub enum AppCommand {
    #[command(about = "Build one app")]
    Build(build::BuildArgs),
}

pub fn run(args: AppArgs) -> anyhow::Result<()> {
    match args.command {
        AppCommand::Build(args) => build::run(args),
    }
}
