use std::{fs, path::Path, process::Command};

use anyhow::Context;

use crate::{config::platform::Dtb, tasks::utils::cmd_echo};

pub const DEVICE_TREE_OUTPUT_PATH: &str = "build/generated/device-tree/platform.dtb";

pub fn materialize(dtb: Option<&Dtb>) -> anyhow::Result<()> {
    let output = Path::new(DEVICE_TREE_OUTPUT_PATH);
    let temporary = Path::new("build/generated/device-tree/platform.dtb.tmp");
    let output_dir = output
        .parent()
        .expect("fixed device-tree output must have a parent");
    fs::create_dir_all(output_dir).context("failed to create device-tree output directory")?;
    if output.exists() {
        fs::remove_file(output).context("failed to remove stale platform DTB")?;
    }
    if temporary.exists() {
        fs::remove_file(temporary).context("failed to remove stale temporary platform DTB")?;
    }

    let Some(dtb) = dtb else {
        return Ok(());
    };

    let workspace_root = std::env::current_dir()?
        .canonicalize()
        .context("failed to canonicalize workspace root")?;
    let source = workspace_root
        .join(&dtb.source)
        .canonicalize()
        .with_context(|| format!("failed to resolve platform DTS `{}`", dtb.source))?;
    if !source.starts_with(&workspace_root) {
        anyhow::bail!("platform DTS `{}` escapes the workspace", dtb.source);
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
        if temporary.exists() {
            fs::remove_file(temporary)
                .context("dtc failed and its partial platform DTB could not be removed")?;
        }
        anyhow::bail!("dtc exited with status: {status}");
    }
    fs::rename(temporary, output).context("failed to publish platform DTB")?;
    Ok(())
}
