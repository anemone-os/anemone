use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, bail};
use clap::Args;
use serde::Serialize;

use crate::{
    config::{app::App as AppManifest, rootfs::Rootfs},
    log_progress,
    tasks::app::build::{BuildCtx, BuiltArtifactInfo, build_app},
};

#[derive(Args)]
pub struct MkfsArgs {
    #[arg(short, long)]
    #[arg(help = "Path to the rootfs manifest file")]
    config: String,

    #[arg(long)]
    #[arg(
        help = "Run the host-side image builder through sudo when libguestfs needs elevated privileges"
    )]
    sudo: bool,
}

pub fn run(args: MkfsArgs) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(&args.config)
        .with_context(|| format!("Failed to read rootfs manifest from '{}'", args.config))?;
    let rootfs = Rootfs::from_str(&content)?;

    let output_dir = Path::new("build/rootfs").join(&rootfs.build.name);
    let staging_dir = output_dir.join("root");
    let image_path = output_dir.join("rootfs.img");

    if output_dir.exists() {
        fs::remove_dir_all(&output_dir)?;
    }
    fs::create_dir_all(&staging_dir)?;

    log_progress!(
        "ROOTFS",
        &format!("Preparing rootfs '{}'", rootfs.build.name)
    );

    RootfsCtx::new(&rootfs, &staging_dir).mkfs()?;

    log_progress!("MKFS", &format!("Generating {}", image_path.display()));
    if args.sudo {
        log_progress!(
            "MKFS",
            "Using sudo for host-side image materialization; you may be prompted for your password"
        );
    }
    rootfs
        .fs
        .fstype
        .mkfs(
            &staging_dir,
            &image_path,
            rootfs.fs.size.as_deref(),
            args.sudo,
        )?;

    Ok(())
}

struct RootfsCtx<'a> {
    rootfs: &'a Rootfs,
    staging_dir: &'a Path,
}

impl<'a> RootfsCtx<'a> {
    fn new(rootfs: &'a Rootfs, staging_dir: &'a Path) -> Self {
        Self {
            rootfs,
            staging_dir,
        }
    }

    fn mkfs(&self) -> anyhow::Result<()> {
        self.stage_base_tree()?;
        self.stage_dirs()?;
        self.stage_apps()?;
        self.stage_files()?;
        self.gen_init_config()?;

        log_progress!("ROOTFS", "Rootfs staging complete");

        Ok(())
    }

    fn stage_base_tree(&self) -> anyhow::Result<()> {
        let Some(base) = &self.rootfs.fs.base else {
            return Ok(());
        };

        fn state_base_dir(staging_dir: &Path, dir: &Path, is_root: bool) -> anyhow::Result<()> {
            if !staging_dir.exists() {
                fs::create_dir_all(staging_dir)?;
            }

            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                if is_root && entry.file_name() == "lost+found" {
                    continue;
                }
                let path = entry.path();
                let relative_path = path.strip_prefix(dir)?;
                let dest_path = staging_dir.join(relative_path);
                if entry.metadata()?.is_file() {
                    if let Some(parent) = dest_path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::copy(&path, &dest_path)?;
                } else if entry.metadata()?.is_dir() {
                    state_base_dir(&dest_path, &path, false)?;
                }
            }
            Ok(())
        }

        log_progress!("ROOTFS", &format!("Copying base tree from '{}'", base));
        state_base_dir(&self.staging_dir, Path::new(base), true)
    }

    fn stage_dirs(&self) -> anyhow::Result<()> {
        for dir in &self.rootfs.dirs {
            log_progress!("ROOTFS", &format!("Creating directory '{}'", dir.path));
            fs::create_dir_all(self.rootfs_path(&dir.path))?;
        }

        Ok(())
    }

    fn stage_apps(&self) -> anyhow::Result<()> {
        let build_ctx = BuildCtx::new(self.rootfs.build.arch.clone())?;

        for app in &self.rootfs.apps {
            log_progress!("ROOTFS", &format!("Staging app '{}'", app.name));

            let installed_dir = self.rootfs_path(&app.installed_dir);

            std::fs::create_dir_all(&installed_dir)?;

            let artifacts = build_app(&app.name, &[], &build_ctx, false)?;
            for artifact in artifacts {
                let dest = installed_dir.join(artifact.name().unwrap());
                std::fs::copy(artifact.output_path, &dest)?;
            }
        }

        Ok(())
    }

    fn stage_files(&self) -> anyhow::Result<()> {
        for file in &self.rootfs.files {
            log_progress!("ROOTFS", &format!("Staging file '{}'", file.source));

            let src = Path::new(&file.source);
            if !src.exists() {
                bail!("rootfs file source '{}' does not exist", src.display());
            }
            if !src.is_file() {
                bail!("rootfs file source '{}' is not a file", src.display());
            }
            let dest = self.rootfs_path(&file.installed_path);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(src, &dest)?;
        }

        Ok(())
    }

    fn rootfs_path(&self, path: &str) -> PathBuf {
        // Skip "/" otherwise joining an absolute manifest path would discard
        // the staging root and write to the host path.
        self.staging_dir.join(
            Path::new(path)
                .components()
                .skip_while(|c| matches!(c, Component::RootDir))
                .collect::<PathBuf>(),
        )
    }

    fn gen_init_config(&self) -> anyhow::Result<()> {
        log_progress!("ROOTFS", "Generating init config");

        // simply a copy. we can add more processing later if needed
        let init_config = self.staging_dir.join(".anemone").join("init");
        if let Some(parent) = init_config.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&init_config, &self.rootfs.init.path)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        platform::Arch,
        rootfs::{Build, Dir, Fs, FsType, Init, Rootfs},
    };

    #[test]
    fn stage_dirs_creates_manifest_paths_under_staging_root() {
        let staging_dir =
            std::env::temp_dir().join(format!("anemone-xtask-rootfs-dirs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&staging_dir);
        fs::create_dir_all(&staging_dir).unwrap();

        let rootfs = Rootfs {
            build: Build {
                name: "dir-test".to_string(),
                arch: Arch::RiscV64,
            },
            fs: Fs {
                fstype: FsType::Ext4,
                base: None,
                size: None,
            },
            init: Init {
                path: "/sbin/init".to_string(),
            },
            apps: Vec::new(),
            dirs: vec![
                Dir {
                    path: "/dev".to_string(),
                },
                Dir {
                    path: "mnt/nested".to_string(),
                },
            ],
            files: Vec::new(),
        };

        let ctx = RootfsCtx::new(&rootfs, &staging_dir);
        ctx.stage_dirs().unwrap();

        assert!(staging_dir.join("dev").is_dir());
        assert!(staging_dir.join("mnt/nested").is_dir());

        fs::remove_dir_all(&staging_dir).unwrap();
    }
}
