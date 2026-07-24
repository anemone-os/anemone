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
    materialize_busybox()?;

    // PID 1 is already its session and process-group leader. Boot installed
    // stdin on the selected Terminal, so only the controlling relation is
    // missing before the fixed BusyBox shell takes over this process.
    tiocsctty(STDIN_FILENO as u32, 0)?;

    let argv = args().collect::<Vec<_>>();
    if argv.len() < 2 {
        return Err(EINVAL);
    }
    execve(BUSYBOX_PATH, &argv, BUSYBOX_ENV)?;
    unreachable!();
}
