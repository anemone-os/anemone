//! The most important tasks provided by `xtask`.
//!
//! Build Anemone kernel for targeted platforms
//! (e.g., QEMU, or real hardware), and produce bootable images.

use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use clap::Args;
use xshell::Shell;

use crate::{
    config::{
        kconfig::Profile,
        platform::DtbType,
        reference::KernelConfigRef,
        resolve::{BuildPresentation, ConfigLoader, ResolvedSystemBuild},
    },
    log_progress,
    tasks::{app::build::build_app, qemu::gen_qemu_cmd, utils::cmd_echo},
    warn,
    workspace::*,
};

mod kernel_output;
pub mod symtab;

#[derive(Args)]
pub struct BuildArgs {
    #[arg(short, long, default_value = KCONFIG_PATH)]
    #[arg(help = "Path to the kconfig file")]
    pub kconfig: String,
}

pub fn run(args: BuildArgs) -> anyhow::Result<()> {
    log_progress!("BUILD", "Starting build process");

    log_progress!("RESOLVE", "Resolving legacy build selection");
    let kernel_config_ref = KernelConfigRef::new(args.kconfig)?;
    let action = ConfigLoader::new(Path::new(".")).resolve_legacy_build(kernel_config_ref)?;
    log_progress!(
        "RESOLVE",
        &format!(
            "selection source={} target={} platform={} kernel-config={} profile={} platform-output={}",
            action.selection_source.as_str(),
            action.system.target_ref,
            action.system.platform_ref,
            action.system.kernel_config_ref,
            action.system.profile.as_str(),
            action
                .system
                .platform
                .uboot
                .as_ref()
                .map(|uboot| uboot.filename.as_str())
                .unwrap_or("elf-only")
        )
    );

    let context = BuildContext::new(action.system, action.presentation);
    context.build()?;

    Ok(())
}

struct BuildContext {
    resolved: ResolvedSystemBuild,
    presentation: BuildPresentation,
}

impl BuildContext {
    fn new(resolved: ResolvedSystemBuild, presentation: BuildPresentation) -> Self {
        Self {
            resolved,
            presentation,
        }
    }

    fn build(&self) -> anyhow::Result<()> {
        log_progress!("PREBUILD", "Preparing build environment");
        self.prebuild()?;
        log_progress!("BUILD", "Building kernel");
        self.build_main()?;
        log_progress!("POSTBUILD", "Finalizing build process");
        self.postbuild()
    }

    fn build_main(&self) -> anyhow::Result<()> {
        self.build_kernel()?;
        Ok(())
    }
    fn prebuild(&self) -> anyhow::Result<()> {
        if std::fs::exists("target")? {
            warn!(
                "WARN",
                "Rebuilding with cargo cache. Some changes might not be reflected."
            );
        }

        Shell::new()?
            .cmd("mkdir")
            .arg("-p")
            .arg("build/generated")
            .run_echo()?;
        Shell::new()?
            .cmd("mkdir")
            .arg("-p")
            .arg("build/apps")
            .run_echo()?;

        self.gen_rust_defs()?;
        self.gen_kernel_lds()?;

        if let Some(dtb) = &self.resolved.platform.dtb {
            match dtb.typ {
                DtbType::Qemu => {
                    log_progress!("DTB", "Generating DTB from qemu");
                    if let Some(qemu) = &self.resolved.platform.qemu {
                        let mut cmd = gen_qemu_cmd(qemu, None);
                        cmd.arg("-machine")
                            .arg(String::from("dumpdtb=anemone-kernel/src/") + dtb.path.as_str());
                        cmd_echo(&cmd);
                        match cmd.status() {
                            Ok(status) => {
                                if !status.success() {
                                    anyhow::bail!("QEMU exited with status: {}", status);
                                }
                                log_progress!("DTB", "Successfully generated DTB from qemu");
                            },
                            Err(e) => {
                                log_progress!(
                                    "ERROR",
                                    &format!("Failed to generate DTB from QEMU: {}", e)
                                );
                                anyhow::bail!("Failed to generate DTB from QEMU: {}", e);
                            },
                        }
                    } else {
                        log_progress!(
                            "ERROR",
                            "QEMU configuration is required to generate DTB from QEMU"
                        );
                        anyhow::bail!("QEMU configuration is required to generate DTB from QEMU")
                    }
                },
                DtbType::File => {
                    todo!();
                },
            }
        }

        Ok(())
    }

    fn gen_rust_defs(&self) -> anyhow::Result<()> {
        let kconfig_defs = self.resolved.kernel_config.parameters.gen_kconfig_defs();
        let platform_defs = self
            .resolved
            .platform
            .gen_platform_defs(&self.resolved.target.root);
        // write to both loader and kernel src directories
        let kconfig_defs_path = format!("anemone-kernel/src/kconfig_defs.rs",);
        let platform_defs_path = format!("anemone-kernel/src/platform_defs.rs",);
        log_progress!("GENDEFS", "Generating kconfig_defs.rs and platform_defs.rs");
        let sh = Shell::new()?;
        sh.write_file(&kconfig_defs_path, &kconfig_defs)?;
        sh.write_file(&platform_defs_path, &platform_defs)?;
        Ok(())
    }

    fn gen_kernel_lds(&self) -> anyhow::Result<()> {
        let lds_template_path = format!(
            "{}/{}/kernel.lds.in",
            ARCH_CONFIGS_PATH,
            self.resolved.platform.build.arch.as_str()
        );
        let lds_template = std::fs::read_to_string(lds_template_path)?;
        let lds_content = lds_template
            .replace(
                "{{KERNEL_LA_BASE}}",
                &format!("0x{:x}", self.resolved.platform.constants.kernel_la_base),
            )
            .replace(
                "{{KERNEL_VA_BASE}}",
                &format!("0x{:x}", self.resolved.platform.constants.kernel_va_base),
            );
        let lds_output_path = format!("build/generated/kernel.lds");
        let sh = Shell::new()?;
        sh.write_file(lds_output_path, lds_content)?;

        Ok(())
    }

    fn build_kernel(&self) -> anyhow::Result<()> {
        log_progress!("COMPILE", "Compiling kernel");
        let sh = Shell::new()?;
        let rustflags = BuildContext::build_rustflags(&[
            "-C",
            "link-arg=-Tbuild/generated/kernel.lds",
            "-C",
            "link-arg=-Map=build/kernel.map",
        ]);
        let mut build = sh
            .with_current_dir("anemone-kernel")
            .cmd("cargo")
            .arg("build")
            .args(&[
                "-Z",
                "build-std=core,alloc",
                "-Z", // Refer to https://github.com/rust-lang/wg-cargo-std-aware/issues/53 for why this is needed
                "build-std-features=compiler-builtins-mem",
            ])
            .args(&["-Z", "json-target-spec"])
            .arg("--target")
            .arg(&format!(
                "../conf/arch/{}/{}.json",
                self.resolved.platform.build.arch.as_str(),
                self.resolved.platform.build.arch.target_triple().as_str()
            ))
            .env("RUSTFLAGS", rustflags);
        for arg in self.resolved.profile.as_cargo_arg() {
            build = build.arg(arg);
        }
        for (feature, enabled) in &self.resolved.kernel_config.features {
            if *enabled {
                build = build.arg("--features").arg(feature);
            }
        }
        build.run_echo()?;

        let built_kernel_path = format!("{}/anemone-kernel", self.cargo_build_dir());
        std::fs::copy(built_kernel_path, "build/anemone.elf")?;

        kernel_output::build_uboot_image(
            &self.resolved.platform.build.arch,
            self.resolved.platform.uboot.as_ref(),
        )?;

        if self.presentation.disasm {
            log_progress!("DISASM", "Generating kernel disassembly");

            let disasm = sh
                .cmd(&self.resolved.platform.build.arch.target_triple().objdump())
                .arg("-d")
                .arg("-S")
                .arg("build/anemone.elf")
                .echo()
                .read()?;
            sh.write_file("build/anemone.disasm", disasm)?;
        }
        Ok(())
    }

    fn postbuild(&self) -> anyhow::Result<()> {
        // currently no-op
        Ok(())
    }
}

impl BuildContext {
    const GENERAL_RUSTFLAGS: &'static [&'static str] =
        &["-C", "force-frame-pointers", "-C", "link-arg=--no-relax"];
    fn cargo_build_dir(&self) -> String {
        format!(
            "target/{}/{}",
            self.resolved.platform.build.arch.target_triple().as_str(),
            match self.resolved.profile {
                Profile::Dev => "debug", // dev builds go to debug/
                Profile::Release => "release",
            },
        )
    }

    fn build_rustflags(flags: &[&str]) -> String {
        let mut all_flags = Self::GENERAL_RUSTFLAGS.to_vec();
        all_flags.extend_from_slice(flags);
        all_flags.join(" ")
    }
}
