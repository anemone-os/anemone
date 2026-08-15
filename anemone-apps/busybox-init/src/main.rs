#![no_std]
#![no_main]

use anemone_rs::{
    abi::{
        fs::linux::{
            STDIN_FILENO,
            at::AT_FDCWD,
            open::{O_CREAT, O_TRUNC, O_WRONLY},
        },
        syscall::{linux::SYS_FCHMODAT, syscall},
    },
    env::args,
    os::linux::{
        fs::{AtFd, Fd, PipeFlags, chdir, close, mkdirat, mount, openat, pipe2, read, write},
        process::{
            Tid, WStatus, WStatusRaw, WaitFor, WaitOptions, execve, exit, fork, getpid, setsid,
            wait4,
        },
        tty::tiocsctty,
    },
    prelude::*,
};

const BUSYBOX_PATH: &str = "/.anemone/busybox";
const APPLET_DIR: &str = "/.anemone/bin";
const SHELL_HOME: &str = "/root";
const SHELL_ARGS: &[&str] = &["busybox", "sh", "-i"];
const SHELL_ENV: &[&str] = &[
    "HOME=/root",
    "PATH=/bin:/sbin:/usr/bin:/usr/sbin",
    "TERM=linux",
];
const INSTALLED_SHELL_ENV: &[&str] = &[
    "HOME=/root",
    "PATH=/.anemone/bin:/bin:/sbin:/usr/bin:/usr/sbin",
    "TERM=linux",
];
const SHELL_START_FAILURE: i8 = 127;

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

fn parse_install_option() -> Result<bool, Errno> {
    let mut argv = args();
    let _program = argv.next();
    match (argv.next(), argv.next()) {
        (None, None) => Ok(false),
        (Some("--install"), None) => Ok(true),
        _ => {
            eprintln!("usage: busybox-init [--install]");
            Err(EINVAL)
        },
    }
}

fn ensure_dir(path: &str, mode: u32) -> Result<(), Errno> {
    match mkdirat(AtFd::Cwd, Path::new(path), mode) {
        Ok(()) | Err(EEXIST) => Ok(()),
        Err(errno) => {
            eprintln!("busybox-init: mkdir {path} failed: {errno}");
            Err(errno)
        },
    }
}

fn mount_fs(source: &str, target: &str, fstype: &str) -> Result<(), Errno> {
    mount(Path::new(source), Path::new(target), fstype).map_err(|errno| {
        eprintln!("busybox-init: mount {fstype} on {target} failed: {errno}");
        errno
    })
}

fn chmod(path: &str, mode: u32) -> Result<(), Errno> {
    // Keep this app-local raw call until anemone-rs exposes fchmodat; the
    // kernel already owns the Linux-compatible syscall and mode validation.
    let mut pathname = path.as_bytes().to_vec();
    pathname.push(0);
    unsafe {
        syscall(
            SYS_FCHMODAT,
            AT_FDCWD as i64 as u64,
            pathname.as_ptr() as u64,
            mode as u64,
            0,
            0,
            0,
        )
    }
    .map(|_| ())
    .map_err(|errno| {
        eprintln!("busybox-init: chmod {path} to {mode:#o} failed: {errno}");
        errno
    })
}

fn prepare_filesystems() -> Result<(), Errno> {
    ensure_dir("/dev", 0o755)?;
    mount_fs("devfs", "/dev", "devfs")?;
    mount_fs("devpts", "/dev/pts", "devpts")?;
    mount_fs("ramfs", "/dev/shm", "ramfs")?;
    chmod("/dev/shm", 0o1777)?;

    ensure_dir("/proc", 0o755)?;
    mount_fs("proc", "/proc", "proc")?;

    ensure_dir("/run", 0o755)?;
    mount_fs("ramfs", "/run", "ramfs")?;
    chmod("/run", 0o755)?;

    ensure_dir("/tmp", 0o1777)?;
    mount_fs("ramfs", "/tmp", "ramfs")?;
    chmod("/tmp", 0o1777)?;
    Ok(())
}

fn install_busybox_applets() -> Result<(), Errno> {
    ensure_dir(APPLET_DIR, 0o755)?;
    match fork()? {
        Some(child) => {
            let mut status = WStatusRaw::EMPTY;
            loop {
                match wait4(
                    WaitFor::ChildWithTgid(child),
                    Some(&mut status),
                    WaitOptions::empty(),
                ) {
                    Ok(Some(waited)) if waited == child => break,
                    Ok(Some(_)) => return Err(ECHILD),
                    Ok(None) => unreachable!("blocking wait4 returned no child"),
                    Err(EINTR) => {},
                    Err(errno) => return Err(errno),
                }
            }
            match status.read() {
                WStatus::Exited(0) => Ok(()),
                status => {
                    eprintln!("busybox-init: applet installation failed with {status:?}");
                    Err(EIO)
                },
            }
        },
        None => {
            // Keep applet names independent from the materialized executable's
            // hard-link count so rebooting a reused test image cannot inflate
            // that inode through BusyBox's replacement install path.
            let argv = &["busybox", "--install", "-s", APPLET_DIR];
            if let Err(errno) = execve(BUSYBOX_PATH, argv, &[]) {
                eprintln!("busybox-init: exec {BUSYBOX_PATH} --install failed: {errno}");
            }
            exit(SHELL_START_FAILURE)
        },
    }
}

fn prepare_shell(install: bool) -> Result<(), Errno> {
    // PID 1 remains outside the controlling-terminal relation so it can reap
    // and replace the shell. Each shell generation owns a fresh session and
    // releases that relation through the ordinary session-leader exit path.
    setsid().map_err(|errno| {
        eprintln!("busybox-init: setsid for shell failed: {errno}");
        errno
    })?;
    tiocsctty(STDIN_FILENO as u32, 0).map_err(|errno| {
        eprintln!("busybox-init: TIOCSCTTY for shell failed: {errno}");
        errno
    })?;
    chdir(SHELL_HOME).map_err(|errno| {
        eprintln!("busybox-init: chdir to {SHELL_HOME} failed: {errno}");
        errno
    })?;
    let env = if install {
        INSTALLED_SHELL_ENV
    } else {
        SHELL_ENV
    };
    execve(BUSYBOX_PATH, SHELL_ARGS, env).map_err(|errno| {
        eprintln!("busybox-init: exec {BUSYBOX_PATH} failed: {errno}");
        errno
    })?;
    unreachable!("execve returned after success")
}

fn report_shell_start_failure(fd: Fd, errno: Errno) {
    let bytes = errno.to_ne_bytes();
    let mut written = 0;
    while written < bytes.len() {
        match write(fd, &bytes[written..]) {
            Ok(0) => return,
            Ok(count) => written += count,
            Err(EINTR) => {},
            Err(_) => return,
        }
    }
}

fn read_shell_start_failure(fd: Fd) -> Result<Option<Errno>, Errno> {
    let mut bytes = [0_u8; size_of::<Errno>()];
    let mut read_count = 0;
    loop {
        match read(fd, &mut bytes[read_count..]) {
            Ok(0) if read_count == 0 => return Ok(None),
            Ok(0) => return Err(EIO),
            Ok(count) => {
                read_count += count;
                if read_count == bytes.len() {
                    return Ok(Some(Errno::from_ne_bytes(bytes)));
                }
            },
            Err(EINTR) => {},
            Err(errno) => return Err(errno),
        }
    }
}

fn reap_child(child: Tid) {
    let mut status = WStatusRaw::EMPTY;
    loop {
        match wait4(
            WaitFor::ChildWithTgid(child),
            Some(&mut status),
            WaitOptions::empty(),
        ) {
            Ok(Some(_)) | Err(ECHILD) => return,
            Ok(None) => unreachable!("blocking wait4 returned no child"),
            Err(EINTR) => {},
            Err(errno) => {
                eprintln!("busybox-init: failed to reap shell {child}: {errno}");
                return;
            },
        }
    }
}

fn spawn_shell(install: bool) -> Result<Tid, Errno> {
    // EOF means exec closed the write end. A pre-exec failure sends errno so
    // PID 1 can fail once instead of entering an unbounded respawn storm.
    let (status_reader, status_writer) = pipe2(PipeFlags::CLOEXEC)?;
    match fork() {
        Ok(Some(child)) => {
            let _ = close(status_writer);
            let result = read_shell_start_failure(status_reader);
            let _ = close(status_reader);
            match result? {
                None => {
                    println!("busybox-init: started shell {child}");
                    Ok(child)
                },
                Some(errno) => {
                    reap_child(child);
                    Err(errno)
                },
            }
        },
        Ok(None) => {
            let _ = close(status_reader);
            if let Err(errno) = prepare_shell(install) {
                report_shell_start_failure(status_writer, errno);
            }
            let _ = close(status_writer);
            exit(SHELL_START_FAILURE)
        },
        Err(errno) => {
            let _ = close(status_reader);
            let _ = close(status_writer);
            Err(errno)
        },
    }
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    if getpid()? != 1 {
        eprintln!("busybox-init: must run as PID 1");
        return Err(EPERM);
    }
    let install = parse_install_option()?;

    prepare_filesystems()?;
    materialize_busybox()?;
    if install {
        install_busybox_applets()?;
    }
    let mut shell = spawn_shell(install)?;

    loop {
        let mut status = WStatusRaw::EMPTY;
        match wait4(WaitFor::AnyChild, Some(&mut status), WaitOptions::empty()) {
            Ok(Some(child)) if child == shell => {
                println!(
                    "busybox-init: shell {child} terminated with {:?}; respawning",
                    status.read()
                );
                shell = spawn_shell(install)?;
            },
            Ok(Some(child)) => {
                println!(
                    "busybox-init: reaped adopted child {child} with {:?}",
                    status.read()
                );
            },
            Ok(None) => unreachable!("blocking wait4 returned no child"),
            Err(EINTR) => {},
            Err(errno) => return Err(errno),
        }
    }
}
