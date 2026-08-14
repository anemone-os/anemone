use clap::{Args, Subcommand};

mod build;
mod driver;

#[derive(Args, Debug)]
pub struct ModuleArgs {
    #[command(subcommand)]
    command: ModuleCommand,
}

#[derive(Subcommand, Debug)]
enum ModuleCommand {
    #[command(about = "Build and export a Nemophila module")]
    Build(BuildArgs),
}

#[derive(Args, Debug)]
struct BuildArgs {
    #[arg(value_name = "IDENTITY")]
    identity: String,
}

pub fn run(args: ModuleArgs) -> anyhow::Result<()> {
    match args.command {
        ModuleCommand::Build(args) => build::run(&args.identity),
    }
}
