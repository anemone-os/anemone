use std::process::Command;

use anyhow::{Context, ensure};

use crate::{
    config::app::{CommandBuild, MAX_COMMAND_ARGUMENTS},
    tasks::app::driver::DriverContext,
};

pub fn build_command(
    build: &CommandBuild,
    ctx: &DriverContext<'_>,
    extra_args: &[String],
) -> anyhow::Result<Command> {
    let total_arguments = build
        .argv
        .len()
        .checked_add(extra_args.len())
        .context("command driver argument count overflow")?;
    ensure!(
        total_arguments <= MAX_COMMAND_ARGUMENTS,
        "command driver for app '{}' has {} arguments after appending extra arguments, exceeding the maximum of {}",
        ctx.app.name,
        total_arguments,
        MAX_COMMAND_ARGUMENTS
    );

    let (program, args) = build
        .argv
        .split_first()
        .context("command driver argv must not be empty")?;
    let mut command = Command::new(program);
    command.args(args);
    command.args(extra_args);
    command.current_dir(ctx.workdir);
    command.env("ANEMONE_ARCH", ctx.context.arch_name());
    command.env(
        "ANEMONE_TARGET_TRIPLE",
        ctx.context.target_triple().as_str(),
    );
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{
            app::{App, Artifact, Build, BuildDriver},
            platform::Arch,
        },
        tasks::app::build::BuildCtx,
    };
    use std::{ffi::OsStr, path::Path};

    fn command_app(argv: &[&str]) -> App {
        App {
            name: "command-test".to_string(),
            build: Build {
                workdir: ".".to_string(),
                driver: BuildDriver::Command(CommandBuild {
                    argv: argv.iter().map(|arg| (*arg).to_string()).collect(),
                }),
            },
            artifacts: vec![Artifact {
                path: "out/command-test".to_string(),
            }],
        }
    }

    #[test]
    fn command_driver_preserves_argv_order_and_injects_target_context() {
        let app = command_app(&["tool", "manifest-arg"]);
        let context = BuildCtx::new(Arch::RiscV64).unwrap();
        let driver = DriverContext {
            app: &app,
            workdir: Path::new("workdir"),
            context: &context,
        };

        let command = build_command(&app_command(&app), &driver, &["extra-arg".to_string()])
            .expect("command should be constructed");
        assert_eq!(command.get_program(), OsStr::new("tool"));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [OsStr::new("manifest-arg"), OsStr::new("extra-arg")]
        );
        assert_eq!(command.get_current_dir(), Some(Path::new("workdir")));

        let environment = command.get_envs().collect::<Vec<_>>();
        assert!(environment.contains(&(OsStr::new("ANEMONE_ARCH"), Some(OsStr::new("riscv64")))));
        assert!(environment.contains(&(
            OsStr::new("ANEMONE_TARGET_TRIPLE"),
            Some(OsStr::new("riscv64-unknown-anemone-elf"))
        )));
    }

    #[test]
    fn command_driver_rejects_total_argv_over_capacity() {
        let app = command_app(&["tool"]);
        let context = BuildCtx::new(Arch::RiscV64).unwrap();
        let driver = DriverContext {
            app: &app,
            workdir: Path::new("."),
            context: &context,
        };
        let extra_args = vec!["arg".to_string(); MAX_COMMAND_ARGUMENTS];

        let error = build_command(&app_command(&app), &driver, &extra_args)
            .unwrap_err()
            .to_string();
        assert!(error.contains("command-test"), "{error}");
        assert!(error.contains("maximum of 256"), "{error}");
    }

    #[test]
    fn command_driver_resolves_program_from_inherited_path() {
        let app = command_app(&[
            "sh",
            "-c",
            "test -n \"$PATH\" && test \"$ANEMONE_ARCH\" = riscv64 && test \"$ANEMONE_TARGET_TRIPLE\" = riscv64-unknown-anemone-elf",
        ]);
        let context = BuildCtx::new(Arch::RiscV64).unwrap();
        let driver = DriverContext {
            app: &app,
            workdir: Path::new("."),
            context: &context,
        };

        let status = build_command(&app_command(&app), &driver, &[])
            .unwrap()
            .status()
            .expect("sh should resolve through inherited PATH");
        assert!(status.success());
    }

    #[test]
    fn command_driver_does_not_interpret_shell_metacharacters() {
        let app = command_app(&["exit 0; printf shell-was-used"]);
        let context = BuildCtx::new(Arch::RiscV64).unwrap();
        let driver = DriverContext {
            app: &app,
            workdir: Path::new("."),
            context: &context,
        };

        let error = build_command(&app_command(&app), &driver, &[])
            .unwrap()
            .status()
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }

    fn app_command(app: &App) -> CommandBuild {
        match &app.build.driver {
            BuildDriver::Command(build) => build.clone(),
            _ => unreachable!(),
        }
    }
}
