//! Run the built OS image in QEMU emulator.
use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    fmt,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    config::{
        platform::{Arch, DtbProvider, DtsAuthority, Qemu, QemuBind, validate_qemu_bindings},
        reference::PlatformRef,
        resolve::ConfigLoader,
        selection::SelectionArgs,
    },
    tasks::utils::{cmd_echo, log_progress},
};
use clap::{Args, Subcommand};

pub const DT_DRIFT_EXIT_STATUS: u8 = 3;

#[derive(Args, Debug)]
#[command(args_conflicts_with_subcommands = true)]
pub struct QemuArgs {
    #[command(subcommand)]
    command: Option<QemuCommand>,

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

#[derive(Subcommand, Debug)]
enum QemuCommand {
    #[command(about = "Maintain a QEMU Platform's device-tree baseline")]
    Dt(DtArgs),
}

#[derive(Args, Debug)]
struct DtArgs {
    #[command(subcommand)]
    command: DtCommand,
}

#[derive(Subcommand, Debug)]
enum DtCommand {
    #[command(about = "Compare or refresh a QEMU-derived DT baseline")]
    Refresh(DtRefreshArgs),
}

#[derive(Args, Debug)]
struct DtRefreshArgs {
    #[arg(long, value_name = "QEMU_PLATFORM")]
    #[arg(help = "Select the QEMU Platform whose DT baseline is maintained")]
    platform: String,

    #[arg(long)]
    #[arg(help = "Check for semantic drift without updating the baseline")]
    check: bool,
}

#[derive(Debug)]
pub struct DtDrift {
    platform: PlatformRef,
}

impl fmt::Display for DtDrift {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "DRIFT: QEMU device tree for platform `{}` differs from its committed baseline",
            self.platform
        )
    }
}

impl std::error::Error for DtDrift {}

pub fn error_exit_status(error: &anyhow::Error) -> u8 {
    if error.downcast_ref::<CleanupFailure>().is_some() {
        1
    } else if error.downcast_ref::<DtDrift>().is_some() {
        DT_DRIFT_EXIT_STATUS
    } else {
        1
    }
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
        .args(
            qemu.args
                .as_ref()
                .map(|args| args.as_slice())
                .unwrap_or(&[]),
        );
    if let Some(cpu) = &qemu.cpu {
        cmd.arg("-cpu").arg(cpu);
    }
    if let Some(bios) = &qemu.bios {
        cmd.arg("-bios").arg(bios);
    }
    if debug {
        cmd.arg("-s").arg("-S");
    }
    cmd.args(expanded_bindings);
    cmd
}

fn qemu_program(arch: &Arch) -> &'static str {
    match arch {
        Arch::RiscV64 => "qemu-system-riscv64",
        Arch::LoongArch64 => "qemu-system-loongarch64",
    }
}

fn gen_qemu_dt_cmd(arch: &Arch, qemu: &Qemu, dump_path: &Path) -> anyhow::Result<Command> {
    let dump_path = dump_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("QEMU DT temporary path is not valid UTF-8"))?;
    if dump_path.contains(',') {
        anyhow::bail!("QEMU DT temporary path must not contain a comma");
    }

    let mut command = Command::new(qemu_program(arch));
    command
        .arg("-machine")
        .arg(format!("{},dumpdtb={dump_path}", qemu.machine));
    if let Some(cpu) = &qemu.cpu {
        command.arg("-cpu").arg(cpu);
    }
    command
        .arg("-smp")
        .arg(qemu.smp.to_string())
        .arg("-m")
        .arg(&qemu.memory);
    if let Some(bios) = &qemu.bios {
        command.arg("-bios").arg(bios);
    }
    Ok(command)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DtMaintenanceAccess {
    ReadWrite,
    CheckOnly,
    None,
}

fn dt_maintenance_access(
    authority: DtsAuthority,
    provider: Option<DtbProvider>,
) -> DtMaintenanceAccess {
    match (authority, provider) {
        (DtsAuthority::ProviderDerived, Some(DtbProvider::Qemu)) => DtMaintenanceAccess::ReadWrite,
        (DtsAuthority::Normative, None) => DtMaintenanceAccess::CheckOnly,
        _ => DtMaintenanceAccess::None,
    }
}

fn run_dt_refresh(args: DtRefreshArgs) -> anyhow::Result<()> {
    let platform_ref = PlatformRef::new(&args.platform)?;
    let platform = ConfigLoader::new(Path::new(".")).load_platform(&platform_ref)?;
    let dtb = platform.dtb.as_ref().ok_or_else(|| {
        anyhow::anyhow!("platform `{platform_ref}` does not declare a device-tree contract")
    })?;
    let qemu = platform.qemu.as_ref().ok_or_else(|| {
        anyhow::anyhow!("platform `{platform_ref}` does not have a QEMU provider")
    })?;
    let access = dt_maintenance_access(dtb.authority, dtb.provider);
    if access == DtMaintenanceAccess::None {
        anyhow::bail!(
            "platform `{platform_ref}` does not allow QEMU DT maintenance: only provider-derived provider=qemu or QEMU-backed normative sources are supported"
        );
    }
    if !args.check && access != DtMaintenanceAccess::ReadWrite {
        anyhow::bail!(
            "platform `{platform_ref}` has a normative DT source; use --check because mutating refresh is not allowed"
        );
    }

    let source = Path::new(&dtb.source);
    let metadata = fs::symlink_metadata(source).map_err(|error| {
        anyhow::anyhow!(
            "failed to inspect DT baseline `{}` for platform `{platform_ref}`: {error}",
            source.display()
        )
    })?;
    if !metadata.file_type().is_file() {
        anyhow::bail!(
            "DT baseline `{}` for platform `{platform_ref}` is not a regular file",
            source.display()
        );
    }

    let temporary = DisposableDirectory::new("anemone-qemu-dt")?;
    let action = (|| -> anyhow::Result<()> {
        let provider_dtb = temporary.path().join("provider.dtb");
        let mut qemu_command = gen_qemu_dt_cmd(&platform.build.arch, qemu, &provider_dtb)?;
        let program = qemu_program(&platform.build.arch);
        log_progress(
            "DT",
            &format!("Materializing QEMU topology for platform `{platform_ref}`"),
        );
        cmd_echo(&qemu_command);
        let status = qemu_command.status().map_err(|error| {
            anyhow::anyhow!("failed to launch `{program}` for DT refresh: {error}")
        })?;
        if !status.success() {
            anyhow::bail!("`{program}` DT refresh exited with status: {status}");
        }
        if !provider_dtb.is_file() {
            anyhow::bail!(
                "`{program}` reported success but did not create {}",
                provider_dtb.display()
            );
        }

        let baseline = canonicalize_dts(source, temporary.path(), "baseline")?;
        let provider = canonicalize_dtb(&provider_dtb, temporary.path(), "provider")?;
        if baseline == provider {
            log_progress(
                "DT",
                &format!("platform `{platform_ref}` baseline matches QEMU provider output"),
            );
            return Ok(());
        }

        print!("{}", semantic_diff(source, &baseline, &provider));
        if args.check {
            return Err(DtDrift {
                platform: platform_ref.clone(),
            }
            .into());
        }

        let provenance = qemu_dt_provenance(&platform.build.arch, qemu);
        let updated = render_provider_baseline(&provider, &provenance);
        atomic_replace(source, updated.as_bytes())?;
        log_progress(
            "DT",
            &format!("Updated provider-derived baseline {}", source.display()),
        );
        Ok(())
    })();
    finish_with_cleanup(action, temporary.close())
}

fn qemu_dt_provenance(arch: &Arch, qemu: &Qemu) -> String {
    let mut tokens = vec![
        qemu_program(arch).to_string(),
        "-machine".to_string(),
        format!("{},dumpdtb=<temp>", qemu.machine),
    ];
    if let Some(cpu) = &qemu.cpu {
        tokens.extend(["-cpu".to_string(), cpu.clone()]);
    }
    tokens.extend([
        "-smp".to_string(),
        qemu.smp.to_string(),
        "-m".to_string(),
        qemu.memory.clone(),
    ]);
    if let Some(bios) = &qemu.bios {
        tokens.extend(["-bios".to_string(), bios.clone()]);
    }
    tokens.join(" ")
}

fn canonicalize_dts(source: &Path, temporary: &Path, label: &str) -> anyhow::Result<String> {
    let compiled = temporary.join(format!("{label}-compiled.dtb"));
    run_dtc("dts", "dtb", source, &compiled, false)?;
    canonicalize_dtb(&compiled, temporary, label)
}

fn canonicalize_dtb(source: &Path, temporary: &Path, label: &str) -> anyhow::Result<String> {
    let decompiled = temporary.join(format!("{label}-decompiled.dts"));
    run_dtc("dtb", "dts", source, &decompiled, true)?;

    let decompiled_text = fs::read_to_string(&decompiled)
        .map_err(|error| anyhow::anyhow!("failed to read {}: {error}", decompiled.display()))?;
    let stripped = strip_chosen_rng_seed(&decompiled_text);
    let stripped_path = temporary.join(format!("{label}-stripped.dts"));
    fs::write(&stripped_path, stripped)
        .map_err(|error| anyhow::anyhow!("failed to write {}: {error}", stripped_path.display()))?;

    let normalized_dtb = temporary.join(format!("{label}-normalized.dtb"));
    run_dtc("dts", "dtb", &stripped_path, &normalized_dtb, false)?;
    let canonical = temporary.join(format!("{label}-canonical.dts"));
    run_dtc("dtb", "dts", &normalized_dtb, &canonical, true)?;

    let content = fs::read_to_string(&canonical)
        .map_err(|error| anyhow::anyhow!("failed to read {}: {error}", canonical.display()))?;
    Ok(format!("{}\n", content.trim_end()))
}

fn run_dtc(
    input_format: &str,
    output_format: &str,
    input: &Path,
    output: &Path,
    sort: bool,
) -> anyhow::Result<()> {
    let mut command = Command::new("dtc");
    command
        .arg("-I")
        .arg(input_format)
        .arg("-O")
        .arg(output_format);
    if sort {
        command.arg("-s");
    }
    command.arg("-o").arg(output).arg(input);
    cmd_echo(&command);
    let status = command
        .status()
        .map_err(|error| anyhow::anyhow!("failed to launch `dtc`: {error}"))?;
    if !status.success() {
        anyhow::bail!(
            "`dtc` failed to convert {} to {} with status: {status}",
            input.display(),
            output.display()
        );
    }
    Ok(())
}

fn strip_chosen_rng_seed(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut depth = 0usize;
    let mut chosen_depth = None;

    for line in input.lines() {
        let trimmed = line.trim();
        let opens = trimmed.matches('{').count();
        let closes = trimmed.matches('}').count();
        if chosen_depth.is_none() && opens != 0 {
            let node = trimmed.split_once('{').map(|(node, _)| node.trim());
            if node.is_some_and(|node| node.split('@').next() == Some("chosen")) {
                chosen_depth = Some(depth + opens);
            }
        }

        let volatile_property = chosen_depth.is_some_and(|chosen| depth == chosen)
            && trimmed.starts_with("rng-seed =")
            && trimmed.ends_with(';');
        if !volatile_property {
            output.push_str(line);
            output.push('\n');
        }

        depth = depth.saturating_add(opens).saturating_sub(closes);
        if chosen_depth.is_some_and(|chosen| depth < chosen) {
            chosen_depth = None;
        }
    }
    output
}

fn semantic_diff(source: &Path, baseline: &str, provider: &str) -> String {
    let before = baseline.lines().collect::<Vec<_>>();
    let after = provider.lines().collect::<Vec<_>>();
    let mut prefix = 0;
    while prefix < before.len() && prefix < after.len() && before[prefix] == after[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < before.len().saturating_sub(prefix)
        && suffix < after.len().saturating_sub(prefix)
        && before[before.len() - 1 - suffix] == after[after.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let context_start = prefix.saturating_sub(3);
    let before_end = before.len().saturating_sub(suffix);
    let after_end = after.len().saturating_sub(suffix);
    let context_end = (before_end + 3).min(before.len());
    let after_context_end = (after_end + 3).min(after.len());
    let mut diff = format!(
        "--- {} (committed canonical baseline)\n+++ QEMU provider (canonical)\n",
        source.display()
    );
    for line in &before[context_start..prefix] {
        diff.push_str(&format!(" {line}\n"));
    }
    for line in &before[prefix..before_end] {
        diff.push_str(&format!("-{line}\n"));
    }
    for line in &after[prefix..after_end] {
        diff.push_str(&format!("+{line}\n"));
    }
    for line in &before[before_end..context_end] {
        diff.push_str(&format!(" {line}\n"));
    }
    if after_context_end > after_end && context_end == before_end {
        for line in &after[after_end..after_context_end] {
            diff.push_str(&format!(" {line}\n"));
        }
    }
    diff
}

fn render_provider_baseline(canonical: &str, provenance: &str) -> String {
    let mut lines = canonical.lines();
    let first = lines.next().unwrap_or("/dts-v1/;");
    let body = lines.collect::<Vec<_>>().join("\n");
    format!(
        "{first}\n\n// Provider-derived conformance baseline.\n// Provider: qemu\n// Command: {provenance}\n// The volatile /chosen/rng-seed property is intentionally omitted.\n\n{body}\n"
    )
}

fn atomic_replace(path: &Path, content: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("DT baseline path has no parent: {}", path.display()))?;
    let name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("DT baseline path has no file name: {}", path.display()))?
        .to_string_lossy();
    let temporary = parent.join(format!(
        ".{name}.refresh-{}-{}",
        std::process::id(),
        unique_timestamp()?
    ));
    let mut file = File::create(&temporary).map_err(|error| {
        anyhow::anyhow!(
            "failed to create atomic DT update {}: {error}",
            temporary.display()
        )
    })?;
    if let Err(error) = (|| -> std::io::Result<()> {
        file.write_all(content)?;
        file.sync_all()?;
        Ok(())
    })() {
        drop(file);
        let action = Err(anyhow::anyhow!(
            "failed to write atomic DT update {}: {error}",
            temporary.display()
        ));
        return finish_with_cleanup(action, remove_atomic_temporary(&temporary));
    }
    drop(file);
    if let Err(error) = fs::rename(&temporary, path) {
        let action = Err(anyhow::anyhow!(
            "failed to atomically replace {}: {error}",
            path.display()
        ));
        return finish_with_cleanup(action, remove_atomic_temporary(&temporary));
    }
    Ok(())
}

fn remove_atomic_temporary(path: &Path) -> anyhow::Result<()> {
    fs::remove_file(path).map_err(|error| {
        anyhow::anyhow!(
            "failed to remove atomic DT temporary file {}: {error}",
            path.display()
        )
    })
}

fn finish_with_cleanup<T>(
    action: anyhow::Result<T>,
    cleanup: anyhow::Result<()>,
) -> anyhow::Result<T> {
    match (action, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(cleanup)) => Err(CleanupFailure {
            action: None,
            cleanup,
        }
        .into()),
        (Err(action), Err(cleanup)) => Err(CleanupFailure {
            action: Some(action),
            cleanup,
        }
        .into()),
    }
}

#[derive(Debug)]
struct CleanupFailure {
    action: Option<anyhow::Error>,
    cleanup: anyhow::Error,
}

impl fmt::Display for CleanupFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(action) = &self.action {
            write!(
                formatter,
                "{action:#}; the action also failed to clean temporary state: {:#}",
                self.cleanup
            )
        } else {
            write!(formatter, "temporary cleanup failed: {:#}", self.cleanup)
        }
    }
}

impl std::error::Error for CleanupFailure {}

fn unique_timestamp() -> anyhow::Result<u128> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| anyhow::anyhow!("system clock is before UNIX epoch: {error}"))?
        .as_nanos())
}

struct DisposableDirectory {
    path: PathBuf,
    cleaned: bool,
}

impl DisposableDirectory {
    fn new(prefix: &str) -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            unique_timestamp()?
        ));
        fs::create_dir(&path).map_err(|error| {
            anyhow::anyhow!(
                "failed to create disposable directory {}: {error}",
                path.display()
            )
        })?;
        Ok(Self {
            path,
            cleaned: false,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn close(mut self) -> anyhow::Result<()> {
        self.cleanup()
    }

    fn cleanup(&mut self) -> anyhow::Result<()> {
        match fs::remove_dir_all(&self.path) {
            Ok(()) => {
                self.cleaned = true;
                Ok(())
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.cleaned = true;
                Ok(())
            },
            Err(error) => Err(anyhow::anyhow!(
                "failed to remove disposable directory {}: {error}",
                self.path.display()
            )),
        }
    }
}

impl Drop for DisposableDirectory {
    fn drop(&mut self) {
        if !self.cleaned
            && let Err(error) = self.cleanup()
        {
            eprintln!("warning: best-effort temporary cleanup failed: {error:#}");
        }
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

pub fn run(mut args: QemuArgs) -> anyhow::Result<()> {
    if let Some(command) = args.command.take() {
        return match command {
            QemuCommand::Dt(args) => match args.command {
                DtCommand::Refresh(args) => run_dt_refresh(args),
            },
        };
    }

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
            cpu: Some("rv64".to_string()),
            smp: 1,
            memory: "1G".to_string(),
            bios: Some("default".to_string()),
            args: Some(vec!["-rtc".to_string(), "base=utc".to_string()]),
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
                "-bios",
                "default",
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
    fn dt_command_uses_only_topology_snapshot() {
        let qemu = Qemu {
            machine: "virt".to_string(),
            cpu: Some("rv64".to_string()),
            smp: 2,
            memory: "2G".to_string(),
            bios: Some("default".to_string()),
            args: Some(vec!["-device".to_string(), "ignored".to_string()]),
            bind: vec![QemuBind {
                name: "ignored".to_string(),
                template: vec!["{{}}".to_string()],
            }],
        };
        let command =
            gen_qemu_dt_cmd(&Arch::RiscV64, &qemu, Path::new("/tmp/provider.dtb")).unwrap();

        assert_eq!(command.get_program(), "qemu-system-riscv64");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                "-machine",
                "virt,dumpdtb=/tmp/provider.dtb",
                "-cpu",
                "rv64",
                "-smp",
                "2",
                "-m",
                "2G",
                "-bios",
                "default",
            ]
            .map(std::ffi::OsStr::new)
        );
    }

    #[test]
    fn dt_maintenance_access_preserves_normative_check_only_target() {
        assert_eq!(
            dt_maintenance_access(DtsAuthority::ProviderDerived, Some(DtbProvider::Qemu)),
            DtMaintenanceAccess::ReadWrite
        );
        assert_eq!(
            dt_maintenance_access(DtsAuthority::Normative, None),
            DtMaintenanceAccess::CheckOnly
        );
        assert_eq!(
            dt_maintenance_access(DtsAuthority::ProviderDerived, Some(DtbProvider::Firmware)),
            DtMaintenanceAccess::None
        );
    }

    #[test]
    fn canonical_text_removes_only_chosen_rng_seed() {
        let source = r#"/dts-v1/;

/ {
	rng-seed = <0x01>;
	chosen {
		stdout-path = "/soc/serial";
		rng-seed = <0x02 0x03>;
		child {
			rng-seed = <0x05>;
		};
	};
	device {
		rng-seed = <0x04>;
	};
};
"#;
        let canonical = strip_chosen_rng_seed(source);

        assert!(canonical.contains("\trng-seed = <0x01>;"));
        assert!(canonical.contains("\t\trng-seed = <0x04>;"));
        assert!(canonical.contains("\t\t\trng-seed = <0x05>;"));
        assert!(!canonical.contains("<0x02 0x03>"));
        assert!(canonical.contains("stdout-path"));
    }

    #[test]
    fn drift_has_dedicated_exit_status_and_semantic_diff() {
        let error = anyhow::Error::new(DtDrift {
            platform: PlatformRef::new("qemu-virt-rv64").unwrap(),
        });
        assert_eq!(error_exit_status(&error), DT_DRIFT_EXIT_STATUS);
        assert_eq!(error_exit_status(&anyhow::anyhow!("tool failure")), 1);

        let diff = semantic_diff(
            Path::new("conf/platforms/test.dts"),
            "/dts-v1/;\n/ {\n\tvalue = <1>;\n};\n",
            "/dts-v1/;\n/ {\n\tvalue = <2>;\n};\n",
        );
        assert!(diff.contains("-\tvalue = <1>;"));
        assert!(diff.contains("+\tvalue = <2>;"));
    }

    #[test]
    fn provider_render_and_atomic_replace_are_deterministic() {
        let workspace = BindWorkspace::new();
        let baseline = workspace.file("baseline.dts");
        let rendered = render_provider_baseline(
            "/dts-v1/;\n\n/ {\n};\n",
            "qemu-system-riscv64 -machine virt,dumpdtb=<temp> -smp 1 -m 1G",
        );
        atomic_replace(&baseline, rendered.as_bytes()).unwrap();
        let actual = fs::read_to_string(&baseline).unwrap();

        assert_eq!(actual, rendered);
        assert!(actual.contains("// Provider: qemu"));
        assert!(actual.contains("dumpdtb=<temp>"));
        assert!(fs::read_dir(&workspace.0).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("refresh-")
        }));
    }

    #[test]
    fn atomic_replace_failure_removes_same_directory_temporary() {
        let workspace = BindWorkspace::new();
        let destination = workspace.0.join("baseline.dts");
        fs::create_dir(&destination).unwrap();

        let error = atomic_replace(&destination, b"/dts-v1/;\n").unwrap_err();
        assert!(format!("{error:#}").contains("failed to atomically replace"));
        assert!(fs::read_dir(&workspace.0).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("refresh-")
        }));
    }

    #[test]
    fn cleanup_failures_participate_in_the_action_result() {
        let drift = anyhow::Error::new(DtDrift {
            platform: PlatformRef::new("qemu-virt-rv64").unwrap(),
        });
        let error = finish_with_cleanup::<()>(Err(drift), Err(anyhow::anyhow!("cleanup failed")))
            .unwrap_err();
        let diagnostic = format!("{error:#}");
        assert!(diagnostic.contains("DRIFT"));
        assert!(diagnostic.contains("cleanup failed"));
        assert_eq!(error_exit_status(&error), 1);

        let disposable = DisposableDirectory::new("anemone-qemu-dt-test").unwrap();
        let path = disposable.path().to_owned();
        fs::remove_dir(&path).unwrap();
        File::create(&path).unwrap();
        let error = disposable.close().unwrap_err();
        assert!(format!("{error:#}").contains("failed to remove disposable directory"));
        fs::remove_file(path).unwrap();
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
