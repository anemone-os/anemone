//! The most important tasks provided by `xtask`.
//!
//! Build Anemone kernel for targeted platforms
//! (e.g., QEMU, or real hardware), and produce bootable images.

use std::{fs, os::unix::fs::PermissionsExt, path::Path};

use anyhow::Context;
use clap::Args;
use xshell::Shell;

use crate::{
    config::{
        build_preset::CargoProfile,
        platform::{Config as PlatformConfig, DtbDelivery, DtbProvider, resolve_qemu_provider},
        resolve::{ConfigLoader, ResolvedSystemBuild},
        selection::{BindArgs, BindValues, SelectionArgs, reject_unconsumed_bindings},
        system_target::InitialProgramSource,
    },
    log_progress,
    tasks::app::build::{BuildCtx, BuiltArtifactInfo, build_app},
    warn,
    workspace::*,
};

mod device_tree;
mod generated_defs;
mod kernel_output;
pub mod symtab;

use generated_defs::{render_embedded_app_projection, render_rootfs_entry_projection};

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
    resolve_build_bindings(&mut action.system.platform, &bindings)?;
    log_progress!(
        "RESOLVE",
        &format!(
            "selection source={}{} target={} target-config={} platform={} platform-config={} kernel-config={} profile={} platform-output={} network={}",
            action.selection_source.as_str(),
            action
                .selection_source
                .config_path()
                .map(|path| format!(" preset-config={}", path.display()))
                .unwrap_or_default(),
            action.system.target_ref,
            action.system.target_path.display(),
            action.system.platform_ref,
            action.system.platform_path.display(),
            action.system.kernel_config_ref,
            action.system.profile.as_str(),
            action
                .system
                .platform
                .uboot
                .as_ref()
                .map(|uboot| uboot.filename())
                .unwrap_or("elf-only"),
            action
                .system
                .target
                .network
                .as_ref()
                .map(|network| format!(
                    "{}={}/{} gateway={}",
                    network.ipv4.interface,
                    network.ipv4.address,
                    network.ipv4.prefix,
                    network
                        .ipv4
                        .default_gateway
                        .map(|gateway| gateway.to_string())
                        .unwrap_or_else(|| "none".to_string())
                ))
                .unwrap_or_else(|| "loopback-only".to_string())
        )
    );

    let context = BuildContext::new(action.system, args.disasm);
    context.build()?;

    Ok(())
}

fn resolve_build_bindings(
    platform: &mut PlatformConfig,
    bindings: &BindValues,
) -> anyhow::Result<()> {
    let materializes_qemu_dtb = matches!(
        platform.dtb.as_ref(),
        Some(dtb)
            if dtb.delivery == DtbDelivery::Embedded
                && dtb.provider == Some(DtbProvider::Qemu)
    );
    if materializes_qemu_dtb {
        let qemu = platform
            .qemu
            .as_ref()
            .expect("validated embedded QEMU DT contract must have a provider");
        let (resolved, consumed) = resolve_qemu_provider(qemu, &bindings, false)?;
        reject_unconsumed_bindings(&bindings, &consumed)?;
        platform.qemu = Some(resolved);
    } else if let Some(name) = bindings.keys().next() {
        anyhow::bail!("unknown or unconsumed bind `{name}`");
    }
    Ok(())
}

struct BuildContext {
    resolved: ResolvedSystemBuild,
    disasm: bool,
}

const fn kernel_link_passes(kernel_symbols: bool) -> usize {
    if kernel_symbols { 2 } else { 1 }
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

        // The discovery pass always sees one canonical valid empty table. A
        // disabled build does not compile the consumer, but keeping this input
        // valid avoids a stale non-empty blob becoming implicit build state.
        symtab::prepare_empty_table()?;

        self.gen_rust_defs()?;
        self.gen_kernel_lds()?;

        log_progress!("DTB", "Applying resolved Platform DT contract");
        device_tree::materialize(&self.resolved.platform)?;

        Ok(())
    }

    fn gen_initial_program_projection(&self) -> anyhow::Result<String> {
        match &self.resolved.target.initial_program {
            InitialProgramSource::RootfsEntry { argv } => {
                log_progress!("BOOT", "initial-program=rootfs-entry");
                Ok(render_rootfs_entry_projection(argv.as_deref()))
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
                render_embedded_app_projection(&artifact.output_path, argv.as_deref())
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
        let kernel_symbols = self
            .resolved
            .kernel_config
            .features
            .get("kernel_symbols")
            .copied()
            .unwrap_or(false);
        let link_passes = kernel_link_passes(kernel_symbols);

        log_progress!(
            "COMPILE",
            if link_passes == 2 {
                "Compiling kernel discovery pass"
            } else {
                "Compiling kernel"
            }
        );
        self.compile_kernel()?;
        let built_kernel_path = format!("{}/anemone-kernel", self.cargo_build_dir());

        if link_passes == 2 {
            std::fs::copy(&built_kernel_path, symtab::DISCOVERY_ELF).with_context(|| {
                format!(
                    "failed to preserve discovery ELF '{}'",
                    symtab::DISCOVERY_ELF
                )
            })?;
            let discovery = symtab::generate_from_discovery(Path::new(symtab::DISCOVERY_ELF))?;
            log_progress!(
                "SYMTAB",
                &format!(
                    "discovery selected {} text/function symbols",
                    discovery.len()
                )
            );

            log_progress!("COMPILE", "Compiling kernel final pass");
            self.compile_kernel()?;
            let stats = symtab::verify_and_publish(
                &discovery,
                Path::new(&built_kernel_path),
                Path::new(symtab::PASS_MAP),
                Path::new("build/anemone.elf"),
                Path::new("build/kernel.map"),
            )?;
            log_progress!(
                "SYMTAB",
                &format!(
                    "verified entries={} strings={} bytes={}",
                    stats.entries, stats.strings, stats.total
                )
            );
        } else {
            std::fs::copy(&built_kernel_path, "build/anemone.elf")?;
            std::fs::copy(symtab::PASS_MAP, "build/kernel.map")?;
        }

        kernel_output::build_uboot_artifact(
            &self.resolved.platform.build.arch,
            self.resolved.platform.uboot.as_ref(),
        )?;

        if self.disasm {
            log_progress!("DISASM", "Generating kernel disassembly");

            let sh = Shell::new()?;
            let disasm = sh
                .cmd(&self.resolved.platform.build.arch.kernel_target().objdump())
                .arg("-d")
                .arg("-S")
                .arg("build/anemone.elf")
                .echo()
                .read()?;
            sh.write_file("build/anemone.disasm", disasm)?;
        }
        Ok(())
    }

    fn compile_kernel(&self) -> anyhow::Result<()> {
        let sh = Shell::new()?;
        let rustflags = BuildContext::build_rustflags(&[
            "-C",
            "link-arg=-Tbuild/generated/kernel.lds",
            "-C",
            &format!("link-arg=-Map={}", symtab::PASS_MAP),
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
            .arg(Path::new("..").join(
                self.resolved
                    .platform
                    .build
                    .arch
                    .kernel_target()
                    .spec_json_path(),
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

impl BuildContext {
    const GENERAL_RUSTFLAGS: &'static [&'static str] =
        &["-C", "force-frame-pointers", "-C", "link-arg=--no-relax"];
    fn cargo_build_dir(&self) -> String {
        format!(
            "target/{}/{}",
            self.resolved.platform.build.arch.kernel_target().as_str(),
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
    use crate::config::{
        platform::{Config as PlatformConfig, TEST_QEMU_PLATFORM},
        reference::{BuildPresetRef, KernelConfigRef, SystemTargetRef},
        selection::SelectionRequest,
    };
    use std::{
        collections::HashMap,
        time::{SystemTime, UNIX_EPOCH},
    };

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
    fn kernel_symbol_feature_selects_exactly_one_or_two_link_passes() {
        assert_eq!(kernel_link_passes(false), 1);
        assert_eq!(kernel_link_passes(true), 2);
    }

    #[test]
    fn firmware_build_rejects_runtime_topology_bindings_without_requiring_them() {
        let mut firmware = PlatformConfig::from_str(
            &TEST_QEMU_PLATFORM
                .replace("smp = \"1\"", "smp = \"{{smp}}\"")
                .replace("memory = \"1G\"", "memory = \"{{memory}}\""),
        )
        .unwrap();
        resolve_build_bindings(&mut firmware, &HashMap::new()).unwrap();

        let bindings = HashMap::from([
            ("smp".to_string(), "8".to_string()),
            ("memory".to_string(), "8G".to_string()),
        ]);
        assert!(resolve_build_bindings(&mut firmware, &bindings).is_err());

        let mut embedded = firmware;
        embedded.build.arch = crate::config::platform::Arch::LoongArch64;
        embedded.dtb.as_mut().unwrap().delivery = DtbDelivery::Embedded;
        resolve_build_bindings(&mut embedded, &bindings).unwrap();
        let qemu = embedded.qemu.unwrap();
        assert_eq!(qemu.smp, "8");
        assert_eq!(qemu.memory, "8G");
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
    fn repository_rv64_preset_and_tuple_resolve_the_same_network_target() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let loader = ConfigLoader::new(&repository);
        let preset = loader
            .resolve_selection(SelectionRequest::explicit_preset(
                BuildPresetRef::new("qemu-virt-rv64-release").unwrap(),
            ))
            .unwrap();
        let tuple = loader
            .resolve_selection(SelectionRequest::explicit_tuple(
                SystemTargetRef::new("qemu-virt-rv64").unwrap(),
                KernelConfigRef::new("conf/kconfs/default.toml").unwrap(),
                CargoProfile::Release,
            ))
            .unwrap();

        let preset_ipv4 = &preset.system.target.network.as_ref().unwrap().ipv4;
        let tuple_ipv4 = &tuple.system.target.network.as_ref().unwrap().ipv4;
        assert_eq!(preset.system.target_ref, tuple.system.target_ref);
        assert_eq!(preset_ipv4.interface, tuple_ipv4.interface);
        assert_eq!(preset_ipv4.address, tuple_ipv4.address);
        assert_eq!(preset_ipv4.prefix, tuple_ipv4.prefix);
        assert_eq!(preset_ipv4.default_gateway, tuple_ipv4.default_gateway);
    }
}
