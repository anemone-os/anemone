use std::process::Command;

use anyhow::Context;

use crate::{config::app::CargoBuild, tasks::app::driver::DriverContext};

pub fn build_command(
    build: &CargoBuild,
    ctx: &DriverContext<'_>,
    extra_args: &[String],
) -> anyhow::Result<Command> {
    let (program, args) = build
        .args
        .split_first()
        .context("cargo driver args must not be empty")?;

    let mut cmd = Command::new("cargo");
    cmd.arg(program);
    cmd.args(args);
    cmd.arg("-Z");
    cmd.arg("build-std=core,alloc");
    cmd.arg("-Z");
    cmd.arg("build-std-features=compiler-builtins-mem");
    cmd.arg("--target");

    let cargo_target = ctx.context.cargo_target().ok_or_else(|| {
        anyhow::anyhow!("cargo driver is Anemone-only and cannot build the host target")
    })?;
    cmd.arg(cargo_target.as_str());
    if let Some((name, value)) = cargo_target.rustflags_env() {
        cmd.env(name, value);
    }
    cmd.args(extra_args);
    cmd.current_dir(ctx.workdir);

    Ok(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{
            app::{App, AppTarget, Artifact, Build, BuildDriver},
            platform::Arch,
        },
        tasks::app::build::BuildCtx,
    };
    use std::{ffi::OsStr, path::Path};

    fn cargo_app(target: Arch) -> App {
        App {
            name: "cargo-test".to_string(),
            targets: vec![AppTarget::Anemone(target)],
            build: Build {
                workdir: ".".to_string(),
                driver: BuildDriver::Cargo(CargoBuild {
                    args: vec!["build".to_string()],
                }),
            },
            artifacts: vec![Artifact {
                path: "out/cargo-test".to_string(),
                targets: None,
            }],
        }
    }

    #[test]
    fn cargo_driver_uses_builtin_riscv_target() {
        let app = cargo_app(Arch::RiscV64);
        let context = BuildCtx::new(Arch::RiscV64).unwrap();
        let driver = DriverContext {
            app: &app,
            workdir: Path::new("workdir"),
            context: &context,
        };

        let command = build_command(&app_cargo(&app), &driver, &[]).unwrap();
        let args = command.get_args().collect::<Vec<_>>();
        assert!(args.windows(2).any(|args| {
            args == [
                OsStr::new("--target"),
                OsStr::new("riscv64gc-unknown-none-elf"),
            ]
        }));
        assert!(
            !args
                .iter()
                .any(|arg| *arg == OsStr::new("json-target-spec"))
        );
    }

    #[test]
    fn cargo_driver_preserves_loongarch_unaligned_boundary() {
        let app = cargo_app(Arch::LoongArch64);
        let context = BuildCtx::new(Arch::LoongArch64).unwrap();
        let driver = DriverContext {
            app: &app,
            workdir: Path::new("workdir"),
            context: &context,
        };

        let command = build_command(&app_cargo(&app), &driver, &[]).unwrap();
        assert!(
            command
                .get_args()
                .collect::<Vec<_>>()
                .windows(2)
                .any(|args| {
                    args == [
                        OsStr::new("--target"),
                        OsStr::new("loongarch64-unknown-none"),
                    ]
                })
        );
        assert!(command.get_envs().any(|(name, value)| {
            name == OsStr::new("CARGO_TARGET_LOONGARCH64_UNKNOWN_NONE_RUSTFLAGS")
                && value == Some(OsStr::new("-C target-feature=-ual"))
        }));
    }

    fn app_cargo(app: &App) -> CargoBuild {
        match &app.build.driver {
            BuildDriver::Cargo(build) => build.clone(),
            _ => unreachable!(),
        }
    }
}
