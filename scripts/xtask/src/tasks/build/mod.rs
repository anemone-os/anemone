//! The most important tasks provided by `xtask`.
//!
//! Build Anemone kernel for targeted platforms
//! (e.g., QEMU, or real hardware), and produce bootable images.

use clap::Args;
use xshell::Shell;

use crate::{
    config::{kconfig::Profile, platform::DtbType, KConfig, PlatformConfig},
    log_progress,
    tasks::{qemu::gen_qemu_cmd, utils::cmd_echo},
    warn,
    workspace::*,
};

pub mod symtab;

#[derive(Args)]
pub struct BuildArgs {
    #[arg(short, long, default_value = KCONFIG_PATH)]
    #[arg(help = "Path to the kconfig file")]
    pub kconfig: String,
}

pub fn run(args: BuildArgs) -> anyhow::Result<()> {
    log_progress!("BUILD", "Starting build process");

    log_progress!("PARSE", "Parsing configuration files");
    let kconfig_content = match std::fs::read_to_string(args.kconfig) {
        Ok(content) => content,
        Err(e) => {
            log_progress!("ERROR", &format!("Failed to read kconfig file: {}", e));
            return Err(e.into());
        },
    };
    let kconfig = KConfig::from_str(&kconfig_content)?;
    let platform_config_path = format!("{}/{}.toml", PLATFORM_CONFIGS_PATH, kconfig.build.platform);
    let platform_config_content = std::fs::read_to_string(platform_config_path)?;
    let platform_config = PlatformConfig::from_str(&platform_config_content)?;

    let context = BuildContext::new(&kconfig, &platform_config)?;
    context.build()?;

    Ok(())
}

struct BuildContext<'a> {
    kconfig: &'a KConfig,
    platform: &'a PlatformConfig,
}

impl<'a> BuildContext<'a> {
    fn new(kconfig: &'a KConfig, platform: &'a PlatformConfig) -> anyhow::Result<Self> {
        Ok(Self { kconfig, platform })
    }

    fn build(&self) -> anyhow::Result<()> {
        log_progress!("PREBUILD", "Preparing build environment");
        self.prebuild()?;
        log_progress!("BUILD", "Building kernel");
        let ret = self.build_main();
        log_progress!("POSTBUILD", "Finalizing build process");
        self.postbuild().expect("postbuild failed");
        ret
    }

    fn build_main(&self) -> anyhow::Result<()> {
        self.build_kernel()?;
        Ok(())
    }
    fn prebuild(&self) -> anyhow::Result<()> {
        if std::fs::exists("build")? {
            std::fs::remove_dir_all("build")?;
            warn!(
                "WARN",
                "Rebuilding with cargo cache. Some changes might not be reflected"
            );
        }
        Shell::new()?
            .cmd("mkdir")
            .arg("-p")
            .arg("build/generated")
            .run_echo()?;

        self.gen_rust_defs()?;
        self.gen_kernel_lds()?;

        if let Some(dtb) = &self.platform.dtb {
            match dtb.typ {
                DtbType::Qemu => {
                    log_progress!("DTB", "Generating DTB from qemu");
                    if let Some(qemu) = &self.platform.qemu {
                        let mut cmd = gen_qemu_cmd(qemu, None);
                        cmd.arg("-machine")
                            .arg(String::from("dumpdtb=anemone-kernel/src/") + dtb.path.as_str());
                        cmd_echo(&cmd);
                        match cmd.status() {
                            Ok(status) => {
                                if !status.success() {
                                    log_progress!("ERROR", "Failed to generate DTB from QEMU");
                                    anyhow::bail!(
                                        "Failed to generate DTB from QEMU, qemu exited with status: {}",
                                        status
                                    );
                                }
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
                        return anyhow::bail!(
                            "QEMU configuration is required to generate DTB from QEMU"
                        );
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
        let kconfig_defs = self.kconfig.parameters.gen_kconfig_defs();
        let platform_defs = self.platform.gen_platform_defs();
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
            self.platform.build.arch.as_str()
        );
        let lds_template = std::fs::read_to_string(lds_template_path)?;
        let lds_content = lds_template
            .replace(
                "{{KERNEL_LA_BASE}}",
                &format!("0x{:x}", self.platform.constants.kernel_la_base),
            )
            .replace(
                "{{KERNEL_VA_BASE}}",
                &format!("0x{:x}", self.platform.constants.kernel_va_base),
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
                self.platform.build.arch.as_str(),
                self.platform.build.target.as_str()
            ))
            .env("RUSTFLAGS", rustflags);
        for arg in self.kconfig.build.profile.as_cargo_arg() {
            build = build.arg(arg);
        }
        for (feature, enabled) in &self.kconfig.features {
            if *enabled {
                build = build.arg("--features").arg(feature);
            }
        }
        build.run_echo()?;

        let built_kernel_path = format!("{}/anemone-kernel", self.cargo_build_dir());
        std::fs::copy(built_kernel_path, "build/anemone.elf")?;

        if self.kconfig.build.disasm {
            log_progress!("DISASM", "Generating kernel disassembly");

            let disasm = sh
                .cmd(&self.platform.build.target.objdump())
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

impl<'a> BuildContext<'a> {
    const GENERAL_RUSTFLAGS: &'static [&'static str] =
        &["-C", "force-frame-pointers", "-C", "link-arg=--no-relax"];
    fn cargo_build_dir(&self) -> String {
        format!(
            "target/{}/{}",
            self.platform.build.target.as_str(),
            match self.kconfig.build.profile {
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
