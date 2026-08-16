//! Rendering for generated kernel configuration, Platform, and boot inputs.

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::Context;
use xshell::Shell;

use crate::{
    config::system_target::{Root, RootSource, StaticIpv4},
    log_progress,
};

use super::BuildContext;

const NEMOPHILA_SNAPSHOTS_DIR: &str = "build/generated/nemophila";
static NEXT_NEMOPHILA_SNAPSHOT: AtomicU64 = AtomicU64::new(0);

impl BuildContext {
    pub(super) fn gen_rust_defs(&self) -> anyhow::Result<()> {
        let kconfig_defs = self.resolved.kernel_config.parameters.gen_kconfig_defs();
        let platform_defs = self.resolved.platform.gen_platform_defs();
        let initial_program = self.gen_initial_program_projection()?;
        let network = render_network_projection(
            self.resolved
                .target
                .network
                .as_ref()
                .map(|value| &value.ipv4),
        );
        let nemophila = self.gen_nemophila_catalog()?;
        let system_target_defs = render_system_target_defs(
            &self.resolved.target.root,
            &initial_program,
            &network,
            &nemophila,
        );
        let kconfig_defs_path = "anemone-kernel/src/kconfig_defs.rs";
        let platform_defs_path = "anemone-kernel/src/platform_defs.rs";
        let system_target_defs_path = "anemone-kernel/src/system_target_defs.rs";
        log_progress!(
            "GENDEFS",
            "Generating kconfig_defs.rs, platform_defs.rs, and system_target_defs.rs"
        );
        let sh = Shell::new()?;
        sh.write_file(kconfig_defs_path, &kconfig_defs)?;
        sh.write_file(platform_defs_path, &platform_defs)?;
        sh.write_file(system_target_defs_path, &system_target_defs)?;
        Ok(())
    }

    fn gen_nemophila_catalog(&self) -> anyhow::Result<String> {
        let limit = self
            .resolved
            .kernel_config
            .parameters
            .nemophila_artifact_max_bytes
            .expect("resolved KernelConfig has a Nemophila artifact limit");
        if self.resolved.target.nemophila.is_empty() {
            return render_nemophila_catalog(&[]);
        }

        let snapshot_dir = fresh_nemophila_snapshot_dir(Path::new(NEMOPHILA_SNAPSHOTS_DIR))?;
        let mut modules = Vec::with_capacity(self.resolved.target.nemophila.len());
        for identity in &self.resolved.target.nemophila {
            let export = crate::tasks::module::build::build(identity).with_context(|| {
                format!("failed to build embedded Nemophila module `{identity}`")
            })?;
            let snapshot = snapshot_embedded_module(identity, export, &snapshot_dir, limit)?;
            modules.push((identity.as_str(), snapshot));
        }
        render_nemophila_catalog(&modules)
    }
}

fn fresh_nemophila_snapshot_dir(root: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(root)?;
    loop {
        let sequence = NEXT_NEMOPHILA_SNAPSHOT.fetch_add(1, Ordering::Relaxed);
        let path = root.join(format!("system-build-{}-{sequence}", std::process::id()));
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to create Nemophila system-build snapshot directory `{}`",
                        path.display()
                    )
                });
            },
        }
    }
}

fn snapshot_embedded_module(
    identity: &str,
    export: crate::tasks::module::build::ModuleExport,
    snapshot_dir: &Path,
    limit: usize,
) -> anyhow::Result<PathBuf> {
    let metadata = std::fs::symlink_metadata(&export.path).with_context(|| {
        format!(
            "failed to inspect Nemophila module `{identity}` export `{}`",
            export.path.display()
        )
    })?;
    if !metadata.file_type().is_file() {
        anyhow::bail!(
            "Nemophila module `{identity}` export `{}` is not an ordinary file",
            export.path.display()
        );
    }
    let byte_count = export.bytes.len();
    if byte_count > limit {
        anyhow::bail!(
            "Nemophila module `{identity}` export is {byte_count} bytes, exceeding KernelConfig limit {limit}"
        );
    }

    // The stable module export remains useful to ordinary callers, but it is
    // replaceable. Pin this invocation's returned byte snapshot at a unique
    // system-build path before generated Rust can refer to it.
    let snapshot = snapshot_dir.join(format!("{identity}.wasm"));
    std::fs::write(&snapshot, &export.bytes).with_context(|| {
        format!(
            "failed to snapshot Nemophila module `{identity}` export into `{}`",
            snapshot.display()
        )
    })?;
    if !std::fs::symlink_metadata(&snapshot)?.file_type().is_file() {
        anyhow::bail!(
            "Nemophila module `{identity}` snapshot `{}` is not an ordinary file",
            snapshot.display()
        );
    }
    Ok(snapshot)
}

pub(super) fn render_nemophila_catalog(
    modules: &[(&str, std::path::PathBuf)],
) -> anyhow::Result<String> {
    let mut entries = String::new();
    for (identity, path) in modules {
        let path = path
            .to_str()
            .context("Nemophila module export path must be valid UTF-8")?;
        entries.push_str(&format!(
            "    EmbeddedModule {{ identity: {identity:?}, bytes: include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../\", {path:?})) }},\n"
        ));
    }
    Ok(format!(
        "pub(crate) const EMBEDDED_MODULES: &[EmbeddedModule] = &[\n{entries}];\n"
    ))
}

pub(super) fn render_network_projection(ipv4: Option<&StaticIpv4>) -> String {
    let deployment = ipv4
        .map(|ipv4| {
            let interface = format!("{:?}", ipv4.interface);
            let address = render_ipv4(ipv4.address.octets());
            let gateway = ipv4
                .default_gateway
                .map(|gateway| format!("Some({})", render_ipv4(gateway.octets())))
                .unwrap_or_else(|| "None".to_string());
            format!(
                "pub(crate) const STATIC_IPV4_DEPLOYMENT: Option<StaticIpv4Deployment> =\n    Some(StaticIpv4Deployment {{\n        interface: {interface},\n        address: {address},\n        prefix: {},\n        default_gateway: {gateway},\n    }});",
                ipv4.prefix
            )
        })
        .unwrap_or_else(|| {
            "pub(crate) const STATIC_IPV4_DEPLOYMENT: Option<StaticIpv4Deployment> = None;"
                .to_string()
        });

    format!("{deployment}\n")
}

fn render_ipv4(octets: [u8; 4]) -> String {
    format!(
        "[{}, {}, {}, {}]",
        octets[0], octets[1], octets[2], octets[3]
    )
}

pub(super) fn render_rootfs_entry_projection(argv: Option<&[String]>) -> String {
    render_initial_program_projection(&format!(
        "InitialProgramSource::RootfsEntry {{ argv: {} }}",
        render_argv(argv)
    ))
}

pub(super) fn render_embedded_app_projection(
    path: &Path,
    argv: Option<&[String]>,
) -> anyhow::Result<String> {
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
    Ok(render_initial_program_projection(&initial_program))
}

fn render_initial_program_projection(initial_program: &str) -> String {
    format!(
        r#"pub(crate) const INITIAL_PROGRAM_SOURCE: InitialProgramSource = {initial_program};
"#
    )
}

fn render_system_target_defs(
    root: &Root,
    initial_program: &str,
    network: &str,
    nemophila: &str,
) -> String {
    let root_mount = match &root.source {
        RootSource::Block { path } => format!(
            "RootMount {{ fstype: {:?}, source: RootSource::Block {{ device: {path:?} }} }}",
            root.fstype
        ),
        RootSource::Pseudo => format!(
            "RootMount {{ fstype: {:?}, source: RootSource::Pseudo }}",
            root.fstype
        ),
    };

    format!(
        r#"// @generated by `xtask build`; do not edit.
use crate::{{
    boot::{{InitialProgramSource, RootMount, RootSource}},
    nemophila::EmbeddedModule,
    net::StaticIpv4Deployment,
}};

pub(crate) const ROOT_MOUNT: RootMount = {root_mount};

{initial_program}
{network}
{nemophila}"#
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_initial_program_projection_is_closed_and_tracks_embedded_bytes() {
        let rootfs = render_rootfs_entry_projection(None);
        assert!(rootfs.contains("InitialProgramSource::RootfsEntry { argv: None }"));
        assert!(!rootfs.contains("include_bytes!"));

        let rootfs_argv = vec!["busybox".to_string(), "sh".to_string()];
        let rootfs = render_rootfs_entry_projection(Some(&rootfs_argv));
        assert!(rootfs.contains("argv: Some(&[\"busybox\", \"sh\"])"));

        let argv = vec!["busybox".to_string(), "sh".to_string()];
        let embedded =
            render_embedded_app_projection(Path::new("build/apps/init/init"), Some(&argv)).unwrap();
        assert!(embedded.contains("InitialProgramSource::EmbeddedApp"));
        assert!(embedded.contains("include_bytes!"));
        assert!(embedded.contains("build/apps/init/init"));
        assert!(embedded.contains("argv: Some(&[\"busybox\", \"sh\"])"));
    }

    #[test]
    fn system_target_defs_combine_typed_owner_inputs() {
        let root = Root {
            fstype: "ext4".to_string(),
            source: RootSource::Block {
                path: "vda".to_string(),
            },
        };
        let defs =
            render_system_target_defs(&root, "initial-program\n", "network\n", "nemophila\n");

        assert!(defs.contains("boot::{InitialProgramSource, RootMount, RootSource}"));
        assert!(defs.contains(
            "RootMount { fstype: \"ext4\", source: RootSource::Block { device: \"vda\" } }"
        ));
        let initial_program = defs.find("\ninitial-program\n").unwrap();
        let network = defs.find("\nnetwork\n").unwrap();
        let nemophila = defs.find("\nnemophila\n").unwrap();
        assert!(initial_program < network && network < nemophila);
        assert!(!defs.contains("Platform"));

        let pseudo_root = Root {
            fstype: "ramfs".to_string(),
            source: RootSource::Pseudo,
        };
        let defs = render_system_target_defs(&pseudo_root, "", "", "");
        assert!(defs.contains("RootMount { fstype: \"ramfs\", source: RootSource::Pseudo }"));
    }

    #[test]
    fn generated_network_projection_is_closed_and_exact() {
        let absent = render_network_projection(None);
        assert!(absent.contains("STATIC_IPV4_DEPLOYMENT: Option<StaticIpv4Deployment> = None"));

        let ipv4 = StaticIpv4 {
            interface: "eth0".to_string(),
            address: "10.0.2.15".parse().unwrap(),
            prefix: 24,
            default_gateway: Some("10.0.2.2".parse().unwrap()),
        };
        let present = render_network_projection(Some(&ipv4));
        assert!(present.contains("interface: \"eth0\""));
        assert!(present.contains("address: [10, 0, 2, 15]"));
        assert!(present.contains("prefix: 24"));
        assert!(present.contains("default_gateway: Some([10, 0, 2, 2])"));
        assert!(!present.contains("Platform"));
        assert!(!present.contains("root"));
    }

    #[test]
    fn generated_nemophila_catalog_preserves_order_and_only_embeds_identity_and_bytes() {
        let modules = vec![
            (
                "first",
                std::path::PathBuf::from("build/generated/nemophila/system-build-1-0/first.wasm"),
            ),
            (
                "second",
                std::path::PathBuf::from("build/generated/nemophila/system-build-1-0/second.wasm"),
            ),
        ];
        let defs = render_nemophila_catalog(&modules).unwrap();
        assert!(
            defs.find("identity: \"first\"").unwrap() < defs.find("identity: \"second\"").unwrap()
        );
        assert!(defs.contains("include_bytes!"));
        assert!(!defs.contains("mtime"));
        assert!(!defs.contains("manifest"));
    }

    #[test]
    fn empty_nemophila_selection_generates_an_empty_catalog() {
        let defs = render_nemophila_catalog(&[]).unwrap();
        assert!(defs.contains("pub(crate) const EMBEDDED_MODULES"));
        assert!(defs.contains("= &[\n];"));
        assert!(!defs.contains("include_bytes!"));
    }

    #[test]
    fn embedded_nemophila_export_fails_closed_for_invalid_inputs() {
        let root =
            std::env::temp_dir().join(format!("anemone-nemophila-catalog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();
        let snapshot_dir = root.join("snapshots");
        std::fs::create_dir(&snapshot_dir).unwrap();
        let export = |path, bytes: &[u8]| crate::tasks::module::build::ModuleExport {
            path,
            bytes: bytes.into(),
        };
        assert!(
            snapshot_embedded_module(
                "missing",
                export(root.join("missing.wasm"), b"fresh"),
                &snapshot_dir,
                8,
            )
            .is_err()
        );

        let directory = root.join("directory.wasm");
        std::fs::create_dir(&directory).unwrap();
        assert!(
            snapshot_embedded_module("directory", export(directory, b"fresh"), &snapshot_dir, 8,)
                .is_err()
        );

        let oversized = root.join("oversized.wasm");
        std::fs::write(&oversized, [0u8; 9]).unwrap();
        assert!(
            snapshot_embedded_module("oversized", export(oversized, &[0u8; 9]), &snapshot_dir, 8,)
                .is_err()
        );

        let valid = root.join("valid.wasm");
        std::fs::write(&valid, b"replaceable stable export").unwrap();
        let snapshot =
            snapshot_embedded_module("valid", export(valid, b"fresh"), &snapshot_dir, 8).unwrap();
        assert_eq!(std::fs::read(snapshot).unwrap(), b"fresh");
        std::fs::remove_dir_all(root).unwrap();
    }
}
