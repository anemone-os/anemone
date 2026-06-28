pub mod cargo;

use std::{path::Path, process::Command};

use crate::{
    config::app::{App, BuildDriver},
    tasks::app::build::BuildCtx,
};

pub struct DriverContext<'a> {
    pub app: &'a App,
    pub workdir: &'a Path,
    pub context: &'a BuildCtx,
}

pub fn build_command(ctx: &DriverContext<'_>, extra_args: &[String]) -> anyhow::Result<Command> {
    match &ctx.app.build.driver {
        BuildDriver::Cargo(build) => cargo::build_command(build, ctx, extra_args),
    }
}
