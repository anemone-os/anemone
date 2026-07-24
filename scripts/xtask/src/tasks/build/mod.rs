//! The most important tasks provided by `xtask`.
//!
//! Build Anemone kernel for targeted platforms
//! (e.g., QEMU, or real hardware), and produce bootable images.

use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    os::unix::fs::PermissionsExt,
    path::Path,
};

use anyhow::Context;
use clap::Args;
use xshell::Shell;

use crate::{
    config::{
        build_preset::CargoProfile,
        platform::resolve_qemu_provider,
        resolve::{ConfigLoader, ResolvedSystemBuild},
        selection::{BindArgs, SelectionArgs, reject_unconsumed_bindings},
        system_target::InitialProgramSource,
    },
    log_progress,
    tasks::app::build::{BuildCtx, BuiltArtifactInfo, build_app},
    warn,
    workspace::*,
};

mod device_tree;
mod kernel_output;
pub mod symtab;

#[derive(Args)]
pub struct BuildArgs {
    #[command(flatten)]
    selection: SelectionArgs,

    #[command(flatten)]
    bindings: BindArgs,

    #[arg(long)]
    #[arg(help = "Generate a disassembly as an action-local presentation output")]
    disasm: bool,
}

pub fn run(args: BuildArgs) -> anyhow::Result<()> {
    log_progress!("BUILD", "Starting build process");

    log_progress!("RESOLVE", "Resolving build selection");
    let mut action =
        ConfigLoader::new(Path::new(".")).resolve_selection(args.selection.into_request()?)?;
    let bindings = args.bindings.into_values()?;
    if let Some(qemu) = action.system.platform.qemu.as_ref() {
        let (resolved, consumed) = resolve_qemu_provider(qemu, &bindings, false)?;
        reject_unconsumed_bindings(&bindings, &consumed)?;
        action.system.platform.qemu = Some(resolved);
    } else if let Some(name) = bindings.keys().next() {
        anyhow::bail!("unknown bind `{name}`");
    }
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
                .map(|uboot| uboot.filename())
                .unwrap_or("elf-only")
        )
    );

    let context = BuildContext::new(action.system, args.disasm);
    context.build()?;

    Ok(())
}

struct BuildContext {
    resolved: ResolvedSystemBuild,
    disasm: bool,
}

impl BuildContext {
    fn new(resolved: ResolvedSystemBuild, disasm: bool) -> Self {
        Self { resolved, disasm }
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

        log_progress!("DTB", "Applying resolved Platform DT contract");
        device_tree::materialize(&self.resolved.platform)?;

        Ok(())
    }

    fn gen_rust_defs(&self) -> anyhow::Result<()> {
        let kconfig_defs = self.resolved.kernel_config.parameters.gen_kconfig_defs();
        let platform_defs = self
            .resolved
            .platform
            .gen_platform_defs(&self.resolved.target.root);
        let boot_defs = self.gen_boot_defs()?;
        // write to both loader and kernel src directories
        let kconfig_defs_path = format!("anemone-kernel/src/kconfig_defs.rs",);
        let platform_defs_path = format!("anemone-kernel/src/platform_defs.rs",);
        let boot_defs_path = "anemone-kernel/src/boot_defs.rs";
        log_progress!(
            "GENDEFS",
            "Generating kconfig_defs.rs, platform_defs.rs, and boot_defs.rs"
        );
        let sh = Shell::new()?;
        sh.write_file(&kconfig_defs_path, &kconfig_defs)?;
        sh.write_file(&platform_defs_path, &platform_defs)?;
        sh.write_file(boot_defs_path, &boot_defs)?;
        Ok(())
    }

    fn gen_boot_defs(&self) -> anyhow::Result<String> {
        match &self.resolved.target.initial_program {
            InitialProgramSource::RootfsEntry { argv } => {
                log_progress!("BOOT", "initial-program=rootfs-entry");
                Ok(render_rootfs_entry_boot_defs(argv.as_deref()))
            },
            InitialProgramSource::EmbeddedApp { app, argv } => {
                log_progress!("BOOT", &format!("initial-program=embedded-app app={app}"));
                let context = BuildCtx::new(self.resolved.platform.build.arch.clone())?;
                let artifacts =
                    build_app(app.as_str(), &[], &context, false).with_context(|| {
                        format!(
                            "failed to prepare embedded app `{app}` for system target `{}`",
                            self.resolved.target_ref
                        )
                    })?;
                let artifact = validate_embedded_artifact(
                    self.resolved.target_ref.as_str(),
                    app.as_str(),
                    &artifacts,
                )?;
                let byte_count = fs::metadata(&artifact.output_path)
                    .with_context(|| {
                        format!(
                            "failed to inspect embedded app `{app}` export `{}` for system target `{}`",
                            artifact.output_path.display(),
                            self.resolved.target_ref
                        )
                    })?
                    .len();
                log_progress!(
                    "BOOT",
                    &format!(
                        "embedded app={} export={} bytes={byte_count}",
                        app,
                        artifact.output_path.display()
                    )
                );
                render_embedded_app_boot_defs(&artifact.output_path, argv.as_deref())
            },
        }
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

        kernel_output::build_uboot_artifact(
            &self.resolved.platform.build.arch,
            self.resolved.platform.uboot.as_ref(),
        )?;

        if self.disasm {
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

fn validate_embedded_artifact<'a>(
    target: &str,
    app: &str,
    artifacts: &'a [BuiltArtifactInfo],
) -> anyhow::Result<&'a BuiltArtifactInfo> {
    let [artifact] = artifacts else {
        anyhow::bail!(
            "system target `{target}` embedded app `{app}` must export exactly one artifact, got {}",
            artifacts.len()
        );
    };
    let metadata = fs::metadata(&artifact.output_path).with_context(|| {
        format!(
            "failed to inspect system target `{target}` embedded app `{app}` export `{}`",
            artifact.output_path.display()
        )
    })?;
    if !metadata.is_file() {
        anyhow::bail!(
            "system target `{target}` embedded app `{app}` export `{}` is not a regular file",
            artifact.output_path.display()
        );
    }
    if metadata.permissions().mode() & 0o111 == 0 {
        anyhow::bail!(
            "system target `{target}` embedded app `{app}` export `{}` has no execute bit",
            artifact.output_path.display()
        );
    }
    Ok(artifact)
}

fn render_rootfs_entry_boot_defs(argv: Option<&[String]>) -> String {
    render_boot_defs(&format!(
        "InitialProgramSource::RootfsEntry {{ argv: {} }}",
        render_argv(argv)
    ))
}

fn render_embedded_app_boot_defs(path: &Path, argv: Option<&[String]>) -> anyhow::Result<String> {
    let path = path
        .to_str()
        .context("embedded app export path must be valid UTF-8")?;
    let path_literal = format!("{path:?}");
    let initial_program = format!(
        r#"InitialProgramSource::EmbeddedApp {{
    bytes: include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../",
        {path_literal}
    )),
    argv: {},
}}"#,
        render_argv(argv)
    );
    Ok(render_boot_defs(&initial_program))
}

fn render_boot_defs(initial_program: &str) -> String {
    format!(
        r#"// @generated by `xtask build`; do not edit.
pub(crate) enum InitialProgramSource {{
    RootfsEntry {{
        argv: Option<&'static [&'static str]>,
    }},
    EmbeddedApp {{
        bytes: &'static [u8],
        argv: Option<&'static [&'static str]>,
    }},
}}

pub(crate) const INITIAL_PROGRAM_SOURCE: InitialProgramSource =
    {initial_program};
"#
    )
}

fn render_argv(argv: Option<&[String]>) -> String {
    argv.map(|argv| {
        format!(
            "Some(&[{}])",
            argv.iter()
                .map(|argument| format!("{argument:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
    .unwrap_or_else(|| "None".to_string())
}

impl BuildContext {
    const GENERAL_RUSTFLAGS: &'static [&'static str] =
        &["-C", "force-frame-pointers", "-C", "link-arg=--no-relax"];
    fn cargo_build_dir(&self) -> String {
        format!(
            "target/{}/{}",
            self.resolved.platform.build.arch.target_triple().as_str(),
            match self.resolved.profile {
                CargoProfile::Dev => "debug", // dev builds go to debug/
                CargoProfile::Release => "release",
            },
        )
    }

    fn build_rustflags(flags: &[&str]) -> String {
        let mut all_flags = Self::GENERAL_RUSTFLAGS.to_vec();
        all_flags.extend_from_slice(flags);
        all_flags.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDirectory(std::path::PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "anemone-xtask-embedded-app-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn artifact(&self, name: &str, mode: u32) -> BuiltArtifactInfo {
            let path = self.0.join(name);
            fs::write(&path, b"artifact").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            BuiltArtifactInfo {
                source_path: path.clone(),
                output_path: path,
            }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn embedded_artifact_requires_one_executable_regular_file() {
        let root = TestDirectory::new();
        let executable = root.artifact("executable", 0o751);
        assert!(validate_embedded_artifact("target", "app", &[executable.clone()]).is_ok());

        assert!(validate_embedded_artifact("target", "app", &[]).is_err());
        assert!(
            validate_embedded_artifact("target", "app", &[executable.clone(), executable.clone()])
                .is_err()
        );
        let non_executable = root.artifact("non-executable", 0o640);
        assert!(validate_embedded_artifact("target", "app", &[non_executable]).is_err());
        let directory = root.0.join("directory");
        fs::create_dir(&directory).unwrap();
        assert!(
            validate_embedded_artifact(
                "target",
                "app",
                &[BuiltArtifactInfo {
                    source_path: directory.clone(),
                    output_path: directory,
                }]
            )
            .is_err()
        );
    }

    #[test]
    fn generated_boot_defs_are_closed_and_track_embedded_bytes() {
        let rootfs = render_rootfs_entry_boot_defs(None);
        assert!(rootfs.contains("InitialProgramSource::RootfsEntry { argv: None }"));
        assert!(!rootfs.contains("include_bytes!"));

        let rootfs_argv = vec!["busybox".to_string(), "sh".to_string()];
        let rootfs = render_rootfs_entry_boot_defs(Some(&rootfs_argv));
        assert!(rootfs.contains("argv: Some(&[\"busybox\", \"sh\"])"));

        let argv = vec!["busybox".to_string(), "sh".to_string()];
        let embedded =
            render_embedded_app_boot_defs(Path::new("build/apps/init/init"), Some(&argv)).unwrap();
        assert!(embedded.contains("InitialProgramSource::EmbeddedApp"));
        assert!(embedded.contains("include_bytes!"));
        assert!(embedded.contains("build/apps/init/init"));
        assert!(embedded.contains("argv: Some(&[\"busybox\", \"sh\"])"));
    }
}
