//! Run the built OS image in QEMU emulator.
use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    path::Path,
};

use crate::{
    config::{
        platform::{
            Arch, Qemu, QemuBind, expand_placeholders, placeholder_names, resolve_qemu_provider,
        },
        resolve::ConfigLoader,
        selection::{BindArgs, BindValues, SelectionArgs, reject_unconsumed_bindings},
    },
    tasks::utils::{cmd_echo, log_progress},
};
use clap::Args;

#[derive(Args, Debug)]
pub struct QemuArgs {
    #[command(flatten)]
    selection: SelectionArgs,

    #[command(flatten)]
    bindings: BindArgs,

    #[arg(long)]
    #[arg(help = "Show the selected Platform's required and optional bindings and exit")]
    show_bindings: bool,

    #[arg(short, long)]
    #[arg(
        help = "Whether to enable QEMU's built-in GDB server for debugging, by default it is disabled",
        default_value = "false"
    )]
    debug: bool,
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
        .arg(&qemu.smp)
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
    values: &BindValues,
    consumed: &mut HashSet<String>,
) -> anyhow::Result<Vec<OsString>> {
    let mut expanded = Vec::new();
    for declaration in declarations {
        if !values.contains_key(&declaration.name) {
            if declaration.optional {
                continue;
            }
            anyhow::bail!("missing bind `{}`", declaration.name);
        }
        for template in &declaration.template {
            expanded.push(OsString::from(expand_placeholders(
                template, values, consumed,
            )?));
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
    let bindings = args.bindings.into_values()?;
    if args.show_bindings {
        if !bindings.is_empty() {
            anyhow::bail!("--show-bindings does not accept bind values");
        }
        let mut provider_names = HashSet::new();
        for value in [&qemu.machine, &qemu.cpu, &qemu.smp, &qemu.memory]
            .into_iter()
            .chain(qemu.bios.iter())
            .chain(qemu.args.iter())
        {
            provider_names.extend(placeholder_names(value)?);
        }
        let mut provider_names = provider_names.into_iter().collect::<Vec<_>>();
        provider_names.sort();
        for name in provider_names {
            println!("{name} = required");
        }
        for binding in &qemu.bind {
            println!(
                "{} = {} {:?}",
                binding.name,
                if binding.optional {
                    "optional"
                } else {
                    "required"
                },
                binding.template
            );
        }
        return Ok(());
    }

    let (qemu, mut consumed) = resolve_qemu_provider(qemu, &bindings, true)?;
    let expanded = expand_bindings(&qemu.bind, &bindings, &mut consumed)?;
    reject_unconsumed_bindings(&bindings, &consumed)?;
    for (name, value) in &bindings {
        log_progress("BIND", &format!("{name}={value}"));
    }

    log_progress("QEMU", "Launching QEMU emulator...");
    let program = qemu_program(&action.system.platform.build.arch);
    let mut cmd = gen_qemu_cmd(
        &action.system.platform.build.arch,
        &qemu,
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
            smp: "1".to_string(),
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
        let declarations = vec![
            QemuBind {
                name: "kernel-image".to_string(),
                optional: false,
                template: vec!["-kernel".to_string(), "{{kernel-image}}".to_string()],
            },
            QemuBind {
                name: "disk-x0".to_string(),
                optional: false,
                template: vec![
                    "-drive".to_string(),
                    "file={{disk-x0}},backup={{disk-x0}},format=raw,if=none,id=x0".to_string(),
                ],
            },
        ];
        let values = HashMap::from([
            ("disk-x0".to_string(), "disk with space.img".to_string()),
            ("kernel-image".to_string(), "kernel.elf".to_string()),
        ]);
        let mut consumed = HashSet::new();
        let expanded = expand_bindings(&declarations, &values, &mut consumed).unwrap();
        assert_eq!(
            expanded,
            vec![
                OsString::from("-kernel"),
                OsString::from("kernel.elf"),
                OsString::from("-drive"),
                OsString::from(
                    "file=disk with space.img,backup=disk with space.img,format=raw,if=none,id=x0",
                ),
            ]
        );
        assert_eq!(
            consumed,
            HashSet::from(["kernel-image".to_string(), "disk-x0".to_string()])
        );
    }

    #[test]
    fn optional_group_is_omitted_atomically_and_required_group_fails() {
        let declarations = vec![
            QemuBind {
                name: "kernel-image".to_string(),
                optional: false,
                template: vec!["-kernel".to_string(), "{{kernel-image}}".to_string()],
            },
            QemuBind {
                name: "disk-x1".to_string(),
                optional: true,
                template: vec!["-drive".to_string(), "file={{disk-x1}}".to_string()],
            },
        ];
        assert!(expand_bindings(&declarations, &HashMap::new(), &mut HashSet::new()).is_err());

        let values = HashMap::from([("kernel-image".to_string(), "kernel.elf".to_string())]);
        let expanded = expand_bindings(&declarations, &values, &mut HashSet::new()).unwrap();
        assert_eq!(
            expanded,
            [OsString::from("-kernel"), OsString::from("kernel.elf")]
        );
    }
}
