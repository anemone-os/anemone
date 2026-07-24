//! Run the built OS image in QEMU emulator.
use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::{
    config::{
        platform::{Arch, Qemu, QemuBind, validate_qemu_bindings},
        resolve::ConfigLoader,
        selection::SelectionArgs,
    },
    tasks::utils::{cmd_echo, log_progress},
};
use clap::Args;

#[derive(Args, Debug)]
pub struct QemuArgs {
    #[command(flatten)]
    selection: SelectionArgs,

    #[arg(long = "bind", value_name = "NAME=PATH")]
    #[arg(help = "Bind a declared QEMU argv slot to a host path")]
    bind: Vec<QemuBindValue>,

    #[arg(long)]
    #[arg(help = "Show the selected Platform's required bindings and exit")]
    show_bindings: bool,

    #[arg(short, long)]
    #[arg(
        help = "Whether to enable QEMU's built-in GDB server for debugging, by default it is disabled",
        default_value = "false"
    )]
    debug: bool,
}

#[derive(Clone, Debug)]
struct QemuBindValue {
    name: String,
    path: PathBuf,
}

impl FromStr for QemuBindValue {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (name, path) = value
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("QEMU bind must use NAME=PATH"))?;
        Ok(Self {
            name: name.to_owned(),
            path: PathBuf::from(path),
        })
    }
}

pub fn gen_qemu_cmd(
    arch: &Arch,
    qemu: &Qemu,
    debug: bool,
    expanded_bindings: &[OsString],
) -> std::process::Command {
    let mut cmd = std::process::Command::new(qemu_program(arch));
    cmd.arg("-machine")
        .arg(&qemu.machine)
        .arg("-smp")
        .arg(qemu.smp.to_string())
        .arg("-m")
        .arg(&qemu.memory)
        .arg("-nographic")
        .arg("-serial")
        .arg("mon:stdio")
        .args(&qemu.args);
    cmd.arg("-cpu").arg(&qemu.cpu);
    if let Some(bios) = &qemu.bios {
        cmd.arg("-bios").arg(bios);
    }
    if debug {
        cmd.arg("-s").arg("-S");
    }
    cmd.args(expanded_bindings);
    cmd
}

pub(crate) fn qemu_program(arch: &Arch) -> &'static str {
    match arch {
        Arch::RiscV64 => "qemu-system-riscv64",
        Arch::LoongArch64 => "qemu-system-loongarch64",
    }
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
    let action =
        ConfigLoader::new(Path::new(".")).resolve_selection(args.selection.into_request()?)?;
    log_progress(
        "RESOLVE",
        &format!(
            "selection source={} target={} platform={} kernel-config={} profile={}",
            action.selection_source.as_str(),
            action.system.target_ref,
            action.system.platform_ref,
            action.system.kernel_config_ref,
            action.system.profile.as_str(),
        ),
    );

    let qemu = action.system.platform.qemu.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "platform `{}` does not support QEMU execution",
            action.system.platform_ref
        )
    })?;
    if args.show_bindings {
        if !args.bind.is_empty() {
            anyhow::bail!("--show-bindings does not accept bind values");
        }
        for binding in &qemu.bind {
            println!("{} = {:?}", binding.name, binding.template);
        }
        return Ok(());
    }

    let values = args
        .bind
        .iter()
        .map(|binding| (binding.name.clone(), binding.path.clone()))
        .collect::<Vec<_>>();
    let expanded = expand_bindings(&qemu.bind, &values)?;
    for binding in &args.bind {
        log_progress(
            "BIND",
            &format!("{}={}", binding.name, binding.path.display()),
        );
    }

    log_progress("QEMU", "Launching QEMU emulator...");
    let program = qemu_program(&action.system.platform.build.arch);
    let mut cmd = gen_qemu_cmd(
        &action.system.platform.build.arch,
        qemu,
        args.debug,
        &expanded,
    );
    cmd_echo(&cmd);
    let status = cmd
        .status()
        .map_err(|error| anyhow::anyhow!("failed to launch `{program}`: {error}"))?;
    if !status.success() {
        anyhow::bail!("`{program}` exited with status: {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qemu_command_uses_action_owned_program_and_presentation_tokens() {
        let qemu = Qemu {
            machine: "virt".to_string(),
            cpu: "rv64".to_string(),
            smp: 1,
            memory: "1G".to_string(),
            bios: None,
            args: vec!["-rtc".to_string(), "base=utc".to_string()],
            bind: Vec::new(),
        };
        let expanded = vec![OsString::from("-kernel"), OsString::from("kernel.elf")];
        let command = gen_qemu_cmd(&Arch::RiscV64, &qemu, true, &expanded);

        assert_eq!(command.get_program(), "qemu-system-riscv64");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                "-machine",
                "virt",
                "-smp",
                "1",
                "-m",
                "1G",
                "-nographic",
                "-serial",
                "mon:stdio",
                "-rtc",
                "base=utc",
                "-cpu",
                "rv64",
                "-s",
                "-S",
                "-kernel",
                "kernel.elf",
            ]
            .map(std::ffi::OsStr::new)
        );

        assert_eq!(qemu_program(&Arch::LoongArch64), "qemu-system-loongarch64");
    }

    #[test]
    fn bind_expansion_is_ordered_and_token_preserving() {
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
    fn bind_expansion_rejects_invalid_maps_and_paths() {
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
