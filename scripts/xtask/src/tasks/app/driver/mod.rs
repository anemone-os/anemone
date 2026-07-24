pub mod cargo;

use std::{path::Path, process::Command};

use anyhow::bail;

use crate::{
    config::app::{App, BuildDriver},
    tasks::app::build::BuildCtx,
};

pub struct DriverContext<'a> {
    pub app: &'a App,
    pub workdir: &'a Path,
    pub context: &'a BuildCtx,
}

pub fn build_command(
    ctx: &DriverContext<'_>,
    extra_args: &[String],
) -> anyhow::Result<Option<Command>> {
    match &ctx.app.build.driver {
        BuildDriver::Cargo(build) => cargo::build_command(build, ctx, extra_args).map(Some),
        BuildDriver::Source(_) => {
            if !extra_args.is_empty() {
                bail!(
                    "source driver for app '{}' does not accept extra arguments",
                    ctx.app.name
                );
            }
            Ok(None)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{
            app::{App, Artifact, Build, SourceBuild},
            platform::Arch,
        },
        tasks::app::build::BuildCtx,
    };

    fn source_app() -> App {
        App {
            name: "prebuilt".to_string(),
            build: Build {
                workdir: ".".to_string(),
                driver: BuildDriver::Source(SourceBuild {}),
            },
            artifacts: vec![Artifact {
                path: "prebuilt".to_string(),
            }],
        }
    }

    #[test]
    fn source_driver_returns_no_command() {
        let app = source_app();
        let context = BuildCtx::new(Arch::RiscV64).unwrap();
        let driver = DriverContext {
            app: &app,
            workdir: Path::new("."),
            context: &context,
        };

        assert!(build_command(&driver, &[]).unwrap().is_none());
    }

    #[test]
    fn source_driver_rejects_cli_extra_args() {
        let app = source_app();
        let context = BuildCtx::new(Arch::RiscV64).unwrap();
        let driver = DriverContext {
            app: &app,
            workdir: Path::new("."),
            context: &context,
        };

        let error = build_command(&driver, &["--release".to_string()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("source driver for app 'prebuilt'"));
        assert!(error.contains("does not accept extra arguments"));
    }
}
