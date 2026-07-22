//! Run the built OS image in QEMU emulator.
use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    path::PathBuf,
};

use crate::{
    config::{
        PlatformConfig,
        platform::{Qemu, QemuBind, validate_qemu_bindings},
    },
    tasks::utils::{cmd_echo, log_progress},
    workspace::*,
};
use anyhow::Ok;
use clap::Args;

#[derive(Args)]
pub struct QemuArgs {
    #[arg(short, long)]
    #[arg(help = "Which platform to emulate")]
    platform: String,

    #[arg(short, long)]
    #[arg(help = "Path to the kernel image to run")]
    image: String,

    #[arg(short, long)]
    #[arg(
        help = "Whether to enable QEMU's built-in GDB server for debugging, by default it is disabled",
        default_value = "false"
    )]
    debug: bool,
}

pub fn gen_qemu_cmd(qemu: &Qemu, args: Option<&QemuArgs>) -> std::process::Command {
    let mut cmd = std::process::Command::new(&qemu.qemu);
    cmd.arg("-machine")
        .arg(&qemu.machine)
        .arg("-smp")
        .arg(qemu.smp.to_string())
        .arg("-m")
        .arg(&qemu.memory)
        .args(
            qemu.args
                .as_ref()
                .map(|args| args.as_slice())
                .unwrap_or(&[]),
        );
    if let Some(cpu) = &qemu.cpu {
        cmd.arg("-cpu").arg(cpu);
    }
    if let Some(args) = args {
        cmd.arg("-kernel").arg(args.image.clone());
    }
    if let Some(bios) = &qemu.bios {
        cmd.arg("-bios").arg(bios);
    }
    if let Some(true) = args.and_then(|arg| Some(arg.debug)) {
        cmd.arg("-s").arg("-S");
    }
    cmd
}

pub fn expand_bindings(
    declarations: &[QemuBind],
    values: &[(String, PathBuf)],
) -> anyhow::Result<Vec<OsString>> {
    validate_qemu_bindings(declarations)?;
    let declared = declarations
        .iter()
        .map(|binding| binding.name.as_str())
        .collect::<HashSet<_>>();
    let mut provided = HashMap::new();
    for (name, path) in values {
        if !declared.contains(name.as_str()) {
            anyhow::bail!("unknown QEMU bind `{name}`");
        }
        if provided.insert(name.as_str(), path.as_path()).is_some() {
            anyhow::bail!("duplicate QEMU bind value `{name}`");
        }
        if path.as_os_str().is_empty() {
            anyhow::bail!("QEMU bind `{name}` path must not be empty");
        }
        if path.to_string_lossy().contains(',') {
            anyhow::bail!("QEMU bind `{name}` path must not contain a comma");
        }
        let metadata = std::fs::metadata(path)
            .map_err(|error| anyhow::anyhow!("invalid QEMU bind `{name}` path: {error}"))?;
        if !metadata.is_file() {
            anyhow::bail!("QEMU bind `{name}` path is not a regular file");
        }
    }

    let mut expanded = Vec::new();
    for declaration in declarations {
        let path = provided
            .get(declaration.name.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing QEMU bind `{}`", declaration.name))?;
        for template in &declaration.template {
            // Preserve each declared token and splice host paths as OsString
            // segments: bind expansion never invokes a shell or splits on
            // whitespace, and declaration order is the argv order contract.
            let mut argument = OsString::new();
            for (index, literal) in template.split("{{}}").enumerate() {
                if index != 0 {
                    argument.push(path);
                }
                argument.push(literal);
            }
            expanded.push(argument);
        }
    }
    Ok(expanded)
}

pub fn run(args: QemuArgs) -> anyhow::Result<()> {
    let config_path = format!("{}/{}.toml", PLATFORM_CONFIGS_PATH, args.platform);
    let config_content = std::fs::read_to_string(config_path)?;
    let config = PlatformConfig::from_str(&config_content)?;
    if let Some(qemu) = &config.qemu {
        log_progress("QEMU", "Launching QEMU emulator...");
        let mut cmd = gen_qemu_cmd(qemu, Some(&args));
        cmd_echo(&cmd);
        match cmd.status() {
            Result::Ok(status) => {
                if !status.success() {
                    anyhow::bail!("QEMU exited with status: {}", status);
                }
            },
            Err(e) => {
                anyhow::bail!("Failed to launch QEMU: {}", e);
            },
        }
    } else {
        anyhow::bail!(
            "QEMU configuration not found for platform {}",
            args.platform
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dormant_bind_expansion_is_ordered_and_token_preserving() {
        let workspace = BindWorkspace::new();
        let disk = workspace.file("disk with space.img");
        let kernel = workspace.file("kernel.elf");
        let declarations = vec![
            QemuBind {
                name: "kernel-image".to_string(),
                template: vec!["-kernel".to_string(), "{{}}".to_string()],
            },
            QemuBind {
                name: "disk-x0".to_string(),
                template: vec![
                    "-drive".to_string(),
                    "file={{}},backup={{}},format=raw,if=none,id=x0".to_string(),
                ],
            },
        ];

        let expanded = expand_bindings(
            &declarations,
            &[
                ("disk-x0".to_string(), disk.clone()),
                ("kernel-image".to_string(), kernel.clone()),
            ],
        )
        .unwrap();
        assert_eq!(
            expanded,
            vec![
                OsString::from("-kernel"),
                kernel.as_os_str().to_owned(),
                OsString::from("-drive"),
                OsString::from(format!(
                    "file={},backup={},format=raw,if=none,id=x0",
                    disk.display(),
                    disk.display()
                )),
            ]
        );
    }

    #[test]
    fn dormant_bind_expansion_rejects_invalid_maps_and_paths() {
        let workspace = BindWorkspace::new();
        let file = workspace.file("disk.img");
        let comma_file = workspace.file("disk,comma.img");
        let missing = workspace.0.join("missing.img");
        let declarations = vec![QemuBind {
            name: "disk-x0".to_string(),
            template: vec!["file={{}}".to_string()],
        }];

        for values in [
            vec![],
            vec![("unknown".to_string(), file.clone())],
            vec![
                ("disk-x0".to_string(), file.clone()),
                ("disk-x0".to_string(), file.clone()),
            ],
            vec![("disk-x0".to_string(), PathBuf::new())],
            vec![("disk-x0".to_string(), missing)],
            vec![("disk-x0".to_string(), workspace.0.clone())],
            vec![("disk-x0".to_string(), comma_file)],
        ] {
            assert!(
                expand_bindings(&declarations, &values).is_err(),
                "{values:?}"
            );
        }

        let duplicate_declarations = vec![
            QemuBind {
                name: "disk-x0".to_string(),
                template: vec!["file={{}}".to_string()],
            },
            QemuBind {
                name: "disk-x0".to_string(),
                template: vec!["backup={{}}".to_string()],
            },
        ];
        assert!(
            expand_bindings(&duplicate_declarations, &[("disk-x0".to_string(), file)]).is_err()
        );
    }

    struct BindWorkspace(PathBuf);

    impl BindWorkspace {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "anemone-xtask-qemu-bind-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        fn file(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, b"fixture").unwrap();
            path
        }
    }

    impl Drop for BindWorkspace {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
