use std::{
    ffi::OsStr,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::Context;

use crate::{
    config::platform::{Config, DtAuthority, DtbDelivery, DtbProvider, Qemu},
    tasks::{qemu::qemu_program, utils::cmd_echo},
};

pub const DEVICE_TREE_OUTPUT_PATH: &str = "build/generated/device-tree/platform.dtb";
const FDT_MAGIC: u32 = 0xd00d_feed;
const FDT_HEADER_SIZE: u64 = 40;

pub fn materialize(platform: &Config) -> anyhow::Result<()> {
    materialize_at(
        platform,
        Path::new(DEVICE_TREE_OUTPUT_PATH),
        OsStr::new(qemu_program(&platform.build.arch)),
    )
}

fn materialize_at(
    platform: &Config,
    output: &Path,
    provider_program: &OsStr,
) -> anyhow::Result<()> {
    let temporary = temporary_path(output)?;
    let output_dir = output
        .parent()
        .ok_or_else(|| anyhow::anyhow!("device-tree output must have a parent"))?;
    fs::create_dir_all(output_dir).context("failed to create device-tree output directory")?;
    cleanup_outputs(output, &temporary)?;

    let action = match platform.dtb.as_ref() {
        None
        | Some(crate::config::platform::Dtb {
            delivery: DtbDelivery::Firmware,
            ..
        }) => Ok(()),
        Some(dtb)
            if dtb.authority == DtAuthority::Normative
                && dtb.provider.is_none()
                && dtb.source.is_some() =>
        {
            compile_source(
                dtb.source.as_deref().expect("matched source presence"),
                &temporary,
            )
        },
        Some(dtb) if dtb.provider == Some(DtbProvider::Qemu) => {
            let qemu = platform
                .qemu
                .as_ref()
                .expect("validated QEMU DT contract must have a provider");
            materialize_qemu_provider(qemu, provider_program, &temporary)
        },
        Some(_) => anyhow::bail!("validated embedded DT contract has no materialization route"),
    };

    if let Err(error) = action {
        return match cleanup_outputs(output, &temporary) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(anyhow::anyhow!(
                "{error:#}; failed to clean device-tree outputs: {cleanup:#}"
            )),
        };
    }

    if temporary.exists() {
        if let Err(error) = fs::rename(&temporary, output).context("failed to publish platform DTB")
        {
            return match cleanup_outputs(output, &temporary) {
                Ok(()) => Err(error),
                Err(cleanup) => Err(anyhow::anyhow!(
                    "{error:#}; failed to clean device-tree outputs: {cleanup:#}"
                )),
            };
        }
    }
    Ok(())
}

fn compile_source(source: &str, temporary: &Path) -> anyhow::Result<()> {
    let workspace_root = std::env::current_dir()?
        .canonicalize()
        .context("failed to canonicalize workspace root")?;
    compile_source_from_root(&workspace_root, source, temporary)
}

fn compile_source_from_root(
    workspace_root: &Path,
    source: &str,
    temporary: &Path,
) -> anyhow::Result<()> {
    let source = workspace_root
        .join(source)
        .canonicalize()
        .with_context(|| format!("failed to resolve platform DTS `{source}`"))?;
    if !source.starts_with(&workspace_root) {
        anyhow::bail!("platform DTS `{}` escapes the workspace", source.display());
    }
    let metadata = fs::metadata(&source)
        .with_context(|| format!("failed to inspect platform DTS `{}`", source.display()))?;
    if !metadata.is_file() {
        anyhow::bail!("platform DTS `{}` is not a regular file", source.display());
    }

    let mut command = Command::new("dtc");
    command
        .arg("-I")
        .arg("dts")
        .arg("-O")
        .arg("dtb")
        .arg("-o")
        .arg(temporary)
        .arg(&source);
    cmd_echo(&command);
    let status = command.status().context("failed to run dtc")?;
    if !status.success() {
        anyhow::bail!("dtc exited with status: {status}");
    }
    validate_dtb(temporary)
}

fn materialize_qemu_provider(
    qemu: &Qemu,
    provider_program: &OsStr,
    temporary: &Path,
) -> anyhow::Result<()> {
    let mut command = qemu_provider_command(qemu, provider_program, temporary)?;
    cmd_echo(&command);
    let status = command.status().map_err(|error| {
        anyhow::anyhow!(
            "failed to launch `{}` for build-time DT materialization: {error}",
            provider_program.to_string_lossy()
        )
    })?;
    if !status.success() {
        anyhow::bail!(
            "`{}` build-time DT materialization exited with status: {status}",
            provider_program.to_string_lossy()
        );
    }
    validate_dtb(temporary)
}

fn qemu_provider_command(
    qemu: &Qemu,
    provider_program: &OsStr,
    temporary: &Path,
) -> anyhow::Result<Command> {
    let dump_path = temporary
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("QEMU DT temporary path is not valid UTF-8"))?;
    if dump_path.contains(',') {
        anyhow::bail!("QEMU DT temporary path must not contain a comma");
    }

    let mut command = Command::new(provider_program);
    command
        .arg("-machine")
        .arg(format!("{},dumpdtb={dump_path}", qemu.machine))
        .arg("-cpu")
        .arg(&qemu.cpu)
        .arg("-smp")
        .arg(&qemu.smp)
        .arg("-m")
        .arg(&qemu.memory);
    if let Some(bios) = &qemu.bios {
        command.arg("-bios").arg(bios);
    }
    Ok(command)
}

fn validate_dtb(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("DT provider did not create `{}`", path.display()))?;
    if !metadata.is_file() {
        anyhow::bail!(
            "DT provider output `{}` is not a regular file",
            path.display()
        );
    }
    if metadata.len() < FDT_HEADER_SIZE {
        anyhow::bail!("DT provider output `{}` is too small", path.display());
    }

    let mut header = [0u8; 8];
    fs::File::open(path)
        .with_context(|| format!("failed to open DT provider output `{}`", path.display()))?
        .read_exact(&mut header)
        .with_context(|| format!("failed to read DT provider output `{}`", path.display()))?;
    let magic = u32::from_be_bytes(header[0..4].try_into().expect("fixed header slice"));
    let total_size =
        u32::from_be_bytes(header[4..8].try_into().expect("fixed header slice")) as u64;
    if magic != FDT_MAGIC || !(FDT_HEADER_SIZE..=metadata.len()).contains(&total_size) {
        anyhow::bail!(
            "DT provider output `{}` has an invalid FDT header",
            path.display()
        );
    }
    Ok(())
}

fn temporary_path(output: &Path) -> anyhow::Result<PathBuf> {
    let name = output
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("device-tree output must have a file name"))?;
    let mut temporary_name = name.to_os_string();
    temporary_name.push(".tmp");
    Ok(output.with_file_name(temporary_name))
}

fn cleanup_outputs(output: &Path, temporary: &Path) -> anyhow::Result<()> {
    remove_file_if_present(output).context("failed to remove stale platform DTB")?;
    remove_file_if_present(temporary).context("failed to remove temporary platform DTB")
}

fn remove_file_if_present(path: &Path) -> anyhow::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use crate::config::platform::Arch;

    use super::*;

    #[test]
    fn firmware_delivery_removes_stale_outputs_without_starting_provider() {
        let workspace = TestWorkspace::new();
        let output = workspace.path.join("platform.dtb");
        fs::write(&output, b"stale").unwrap();
        fs::write(temporary_path(&output).unwrap(), b"partial").unwrap();
        let platform = example_platform();

        materialize_at(&platform, &output, OsStr::new("/bin/false")).unwrap();

        assert!(!output.exists());
        assert!(!temporary_path(&output).unwrap().exists());
    }

    #[test]
    fn physical_embedded_source_is_compiled() {
        let workspace = TestWorkspace::new();
        let temporary = workspace.path.join("platform.dtb.tmp");
        let repository = Path::new("../..").canonicalize().unwrap();
        compile_source_from_root(&repository, "conf/platforms/example.dts", &temporary).unwrap();

        validate_dtb(&temporary).unwrap();
    }

    #[test]
    fn qemu_provider_uses_only_topology_and_cleans_failures() {
        let workspace = TestWorkspace::new();
        let output = workspace.path.join("platform.dtb");
        let platform = embedded_example_platform();
        let qemu = platform.qemu.as_ref().unwrap();
        let command = qemu_provider_command(qemu, OsStr::new("qemu-test"), &output).unwrap();
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                "-machine",
                &format!("virt,dumpdtb={}", output.display()),
                "-cpu",
                "rv64",
                "-smp",
                "3",
                "-m",
                "1G",
                "-bios",
                "default",
            ]
            .map(OsStr::new)
        );

        fs::write(&output, b"stale").unwrap();
        let error = materialize_at(&platform, &output, OsStr::new("/bin/false")).unwrap_err();
        assert!(format!("{error:#}").contains("exited with status"));
        assert!(!output.exists());
        assert!(!temporary_path(&output).unwrap().exists());

        let error = materialize_at(&platform, &output, OsStr::new("/bin/true")).unwrap_err();
        assert!(format!("{error:#}").contains("did not create"));
        assert!(!output.exists());
    }

    #[test]
    fn qemu_provider_rejects_invalid_output_and_atomically_publishes_valid_dtb() {
        let workspace = TestWorkspace::new();
        let output = workspace.path.join("platform.dtb");
        let platform = embedded_example_platform();
        let invalid = workspace.provider_script("printf invalid > \"${2#*,dumpdtb=}\"");

        let error = materialize_at(&platform, &output, invalid.as_os_str()).unwrap_err();
        assert!(format!("{error:#}").contains("too small"));
        assert!(!output.exists());
        assert!(!temporary_path(&output).unwrap().exists());

        let fixture = workspace.path.join("valid.dtb");
        fs::write(&fixture, minimal_fdt()).unwrap();
        let valid = workspace.provider_script(&format!(
            "cp \"{}\" \"${{2#*,dumpdtb=}}\"",
            fixture.display()
        ));
        materialize_at(&platform, &output, valid.as_os_str()).unwrap();

        assert_eq!(fs::read(&output).unwrap(), minimal_fdt());
        assert!(!temporary_path(&output).unwrap().exists());
    }

    fn example_platform() -> Config {
        Config::from_str(
            &fs::read_to_string("../../conf/platforms/example.toml")
                .expect("failed to read example Platform"),
        )
        .unwrap()
    }

    fn embedded_example_platform() -> Config {
        let mut platform = example_platform();
        platform.build.arch = Arch::LoongArch64;
        platform.qemu.as_mut().unwrap().smp = "3".to_string();
        platform.dtb.as_mut().unwrap().delivery = DtbDelivery::Embedded;
        platform
    }

    fn minimal_fdt() -> Vec<u8> {
        let mut header = Vec::new();
        for value in [FDT_MAGIC, 40, 40, 40, 40, 17, 16, 0, 0, 0] {
            header.extend(value.to_be_bytes());
        }
        header
    }

    struct TestWorkspace {
        path: PathBuf,
    }

    impl TestWorkspace {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "anemone-build-dt-test-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&path).unwrap();
            Self { path }
        }

        fn provider_script(&self, action: &str) -> PathBuf {
            let path = self.path.join(format!(
                "provider-{}.sh",
                self.path.read_dir().unwrap().count()
            ));
            fs::write(&path, format!("#!/bin/sh\nset -eu\n{action}\n")).unwrap();
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&path, permissions).unwrap();
            path
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
