//! Anemone rootfs related tasks.

use clap::{Args, Subcommand};

mod mkfs;

#[derive(Args)]
pub struct RootfsArgs {
    #[command(subcommand)]
    command: RootfsCommand,
}

#[derive(Subcommand)]
pub enum RootfsCommand {
    //    #[command(about = "Inspect the contents of the rootfs image")]
    //    Inspect(inspect::InspectArgs),
    #[command(about = "Create a rootfs image from a directory")]
    Mkfs(mkfs::MkfsArgs),
}

pub fn run(args: RootfsArgs) -> anyhow::Result<()> {
    match args.command {
        RootfsCommand::Mkfs(args) => mkfs::run(args),
    }
}
