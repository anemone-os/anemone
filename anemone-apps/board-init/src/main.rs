#![no_std]
#![no_main]

use anemone_rs::{
    fs::OpenOptions,
    io::Read,
    os::linux::{
        fs::{AtFd, chdir, chroot, mkdirat, mount, umount},
        process::{WStatus, WStatusRaw, WaitFor, WaitOptions, execve, exit, fork, getpid, wait4},
    },
    prelude::*,
};
use serde::Deserialize;

const CONFIG_PATH: &str = "board-init.toml";

#[derive(Deserialize)]
struct Config {
    mounts: Vec<Mount>,
    #[serde(default)]
    run: Vec<Run>,
    init: Init,
}

#[derive(Deserialize)]
struct Mount {
    dest: String,
    dev: String,
    fs: String,
}

#[derive(Deserialize)]
struct Run {
    path: String,
    args: Vec<String>,
}

#[derive(Deserialize)]
struct Init {
    root: String,
    path: String,
    args: Vec<String>,
}

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    if getpid()? != 1 {
        eprintln!("board-init: must run as PID 1");
        return Err(EPERM);
    }

    let contents = read_config()?;
    let config: Config = toml::from_str(&contents).map_err(|error| {
        eprintln!("board-init: failed to parse {CONFIG_PATH}: {error}");
        EINVAL
    })?;

    create_dir("/dev")?;
    mount(Path::new("devfs"), Path::new("/dev"), "devfs").map_err(|errno| {
        eprintln!("board-init: failed to mount temporary devfs: {errno}");
        errno
    })?;

    let mounts_result = mount_configured(&config.mounts);
    // Block-backed mounts retain their resolved device handles, so devfs is
    // needed only while the configured mount syscalls resolve device names.
    let umount_result = umount(Path::new("/dev")).map_err(|errno| {
        eprintln!("board-init: failed to unmount temporary devfs: {errno}");
        errno
    });
    mounts_result?;
    umount_result?;

    chroot(config.init.root.as_str()).map_err(|errno| {
        eprintln!(
            "board-init: failed to chroot to {}: {}",
            config.init.root, errno
        );
        errno
    })?;
    chdir("/").map_err(|errno| {
        eprintln!("board-init: failed to chdir to new root: {errno}");
        errno
    })?;

    for command in &config.run {
        run(command)?;
    }

    let args: Vec<&str> = config.init.args.iter().map(String::as_str).collect();
    execve(config.init.path.as_str(), &args, &[]).map_err(|errno| {
        eprintln!(
            "board-init: failed to exec {} with args {:?}: {}",
            config.init.path, config.init.args, errno
        );
        errno
    })?;
    unreachable!("execve returned after success")
}

fn run(command: &Run) -> Result<(), Errno> {
    match fork().map_err(|errno| {
        eprintln!("board-init: failed to fork for {}: {}", command.path, errno);
        errno
    })? {
        None => {
            let args: Vec<&str> = command.args.iter().map(String::as_str).collect();
            if let Err(errno) = execve(command.path.as_str(), &args, &[]) {
                eprintln!(
                    "board-init: failed to exec {} with args {:?}: {}",
                    command.path, command.args, errno
                );
                exit(127);
            }
            unreachable!("execve returned after success")
        },
        Some(child) => {
            let mut status = WStatusRaw::EMPTY;
            let waited = wait4(
                WaitFor::ChildWithTgid(child),
                Some(&mut status),
                WaitOptions::empty(),
            )?;
            if waited != Some(child) {
                eprintln!("board-init: wait4 did not return child {}", child);
                return Err(ECHILD);
            }
            match status.read() {
                WStatus::Exited(0) => Ok(()),
                status => {
                    eprintln!(
                        "board-init: command {} failed with status {:?}",
                        command.path, status
                    );
                    Err(EIO)
                },
            }
        },
    }
}

fn mount_configured(mounts: &[Mount]) -> Result<(), Errno> {
    for entry in mounts {
        create_dir(entry.dest.as_str())?;
        let source = format!("/dev/{}", entry.dev);
        mount(
            Path::new(source.as_str()),
            Path::new(entry.dest.as_str()),
            entry.fs.as_str(),
        )
        .map_err(|errno| {
            eprintln!(
                "board-init: failed to mount {} on {} as {}: {}",
                source, entry.dest, entry.fs, errno
            );
            errno
        })?;
    }
    Ok(())
}

fn create_dir(path: &str) -> Result<(), Errno> {
    match mkdirat(AtFd::Cwd, Path::new(path), 0o755) {
        Ok(()) | Err(EEXIST) => Ok(()),
        Err(errno) => {
            eprintln!("board-init: failed to create directory {path}: {errno}");
            Err(errno)
        },
    }
}

fn read_config() -> Result<String, Errno> {
    let mut file = OpenOptions::new()
        .read(true)
        .open(Path::new(CONFIG_PATH))
        .map_err(|errno| {
            eprintln!("board-init: failed to open {CONFIG_PATH}: {errno}");
            errno
        })?;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|errno| {
            eprintln!("board-init: failed to read {CONFIG_PATH}: {errno}");
            errno
        })?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }

    String::from_utf8(bytes).map_err(|_| {
        eprintln!("board-init: {CONFIG_PATH} is not valid UTF-8");
        EINVAL
    })
}
