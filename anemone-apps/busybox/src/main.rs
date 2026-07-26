#![no_std]
#![no_main]

use anemone_rs::{
    abi::fs::linux::{
        STDIN_FILENO,
        open::{O_CREAT, O_TRUNC, O_WRONLY},
    },
    env::args,
    os::linux::{
        fs::{AtFd, close, openat, write},
        process::execve,
        tty::tiocsctty,
    },
    prelude::*,
};

const BUSYBOX_PATH: &str = "/.anemone/busybox";
const BUSYBOX_ENV: &[&str] = &[
    "HOME=/",
    "PATH=/bin:/sbin:/usr/bin:/usr/sbin",
    "TERM=linux",
];

#[cfg(target_arch = "riscv64")]
const BUSYBOX_IMAGE: &[u8] = include_bytes!("../bin/riscv64/busybox");

#[cfg(target_arch = "loongarch64")]
const BUSYBOX_IMAGE: &[u8] = include_bytes!("../bin/loongarch64/busybox");

struct LauncherOptions {
    materialize: bool,
    init_tty: bool,
    busybox_args: Vec<&'static str>,
}

fn parse_launcher_options() -> Result<LauncherOptions, Errno> {
    let mut input = args();
    input.next().ok_or(EINVAL)?;

    let mut materialize = false;
    let mut init_tty = false;
    let mut busybox_args = None;

    while let Some(argument) = input.next() {
        match argument {
            "--materialize" if !materialize => materialize = true,
            "--init-tty" if !init_tty => init_tty = true,
            "--args" if busybox_args.is_none() => {
                let arguments = input.next().ok_or(EINVAL)?;
                if input.next().is_some() {
                    return Err(EINVAL);
                }
                let parsed = arguments.split_ascii_whitespace().collect::<Vec<_>>();
                if parsed.is_empty() {
                    return Err(EINVAL);
                }
                busybox_args = Some(parsed);
            },
            _ => return Err(EINVAL),
        }
    }

    Ok(LauncherOptions {
        materialize,
        init_tty,
        busybox_args: busybox_args.ok_or(EINVAL)?,
    })
}

fn materialize_busybox() -> Result<(), Errno> {
    let fd = openat(
        AtFd::Cwd,
        Path::new(BUSYBOX_PATH),
        O_WRONLY | O_CREAT | O_TRUNC,
        0o755,
    )?;
    let mut remaining = BUSYBOX_IMAGE;
    while !remaining.is_empty() {
        match write(fd, remaining) {
            Ok(0) => {
                let _ = close(fd);
                return Err(EIO);
            },
            Ok(written) => remaining = &remaining[written..],
            Err(EINTR) => {},
            Err(errno) => {
                let _ = close(fd);
                return Err(errno);
            },
        }
    }
    close(fd)
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    let options = parse_launcher_options()?;

    if options.materialize {
        materialize_busybox()?;
    }

    if options.init_tty {
        // The final launcher is already its session and process-group leader.
        // Boot installed stdin on the selected Terminal, so this explicit
        // option only establishes the missing controlling relation.
        tiocsctty(STDIN_FILENO as u32, 0)?;
    }

    execve(BUSYBOX_PATH, &options.busybox_args, BUSYBOX_ENV)?;
    unreachable!();
}
