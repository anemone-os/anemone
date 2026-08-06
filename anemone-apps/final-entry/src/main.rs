#![no_std]
#![no_main]

use anemone_rs::{
    abi::system::native::power::SHUTDOWN_MAGIC,
    os::{
        anemone::power::shutdown,
        linux::{
            fs::{AtFd, chdir, fstatat, mkdirat, mount},
            process::{Tid, WStatus, WStatusRaw, WaitFor, WaitOptions, execve, exit, fork, wait4},
        },
    },
    prelude::*,
};

const TEST_DIR: &str = "/glibc";
const TEST_ENV: &[&str] = &[
    "HOME=/root",
    "PATH=/root/.cargo/bin:/usr/local/bin:/usr/bin:/bin:/sbin:/usr/sbin",
    "TERM=linux",
];

// The final judge currently grades only these glibc groups from its root disk.
const TEST_SCRIPTS: &[&str] = &["./cagent_testcode.sh", "./buildstorm_testcode.sh"];

fn ensure_dir(path: &str, mode: u32) -> Result<(), Errno> {
    match fstatat(AtFd::Cwd, Path::new(path)) {
        Ok(_) => return Ok(()),
        Err(ENOENT) => {},
        Err(errno) => {
            eprintln!("final-entry: stat {path} failed: {errno}");
            return Err(errno);
        },
    }

    mkdirat(AtFd::Cwd, Path::new(path), mode).map_err(|errno| {
        eprintln!("final-entry: mkdir {path} failed: {errno}");
        errno
    })
}

fn mount_fs(source: &str, target: &str, fstype: &str) -> Result<(), Errno> {
    mount(Path::new(source), Path::new(target), fstype).map_err(|errno| {
        eprintln!("final-entry: mount {fstype} on {target} failed: {errno}");
        errno
    })
}

fn prepare_filesystems() -> Result<(), Errno> {
    ensure_dir("/dev", 0o755)?;
    mount_fs("devfs", "/dev", "devfs")?;
    ensure_dir("/dev/shm", 0o1777)?;
    mount_fs("ramfs", "/dev/shm", "ramfs")?;

    ensure_dir("/proc", 0o755)?;
    mount_fs("proc", "/proc", "proc")?;

    ensure_dir("/run", 0o755)?;
    mount_fs("ramfs", "/run", "ramfs")?;

    ensure_dir("/tmp", 0o1777)?;
    mount_fs("ramfs", "/tmp", "ramfs")?;
    Ok(())
}

fn wait_for_child(child: Tid) -> Result<WStatus, Errno> {
    let mut status = WStatusRaw::EMPTY;
    loop {
        match wait4(
            WaitFor::ChildWithTgid(child),
            Some(&mut status),
            WaitOptions::empty(),
        ) {
            Ok(Some(waited)) if waited == child => return Ok(status.read()),
            Ok(Some(_)) => return Err(ECHILD),
            Ok(None) => unreachable!("blocking wait4 returned no child"),
            Err(EINTR) => {},
            Err(errno) => return Err(errno),
        }
    }
}

fn run_script(script: &str) -> Result<WStatus, Errno> {
    match fork()? {
        Some(child) => wait_for_child(child),
        None => {
            if let Err(errno) = chdir(TEST_DIR) {
                eprintln!("final-entry: chdir to {TEST_DIR} failed: {errno}");
                exit(127)
            }
            if let Err(errno) = execve(script, &[script], TEST_ENV) {
                eprintln!("final-entry: failed to start {script}: {errno}");
            }
            exit(127)
        },
    }
}

fn run_final_tests() {
    for script in TEST_SCRIPTS {
        println!("final-entry: starting {script}");
        // Each group is scored independently, so one failure must not suppress
        // the later group or the mandatory shutdown path.
        match run_script(script) {
            Ok(status) => println!("final-entry: {script} finished with {status:?}"),
            Err(errno) => eprintln!("final-entry: {script} failed: {errno}"),
        }
    }
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    match prepare_filesystems() {
        Ok(()) => run_final_tests(),
        Err(errno) => eprintln!("final-entry: filesystem setup failed: {errno}"),
    }

    shutdown(SHUTDOWN_MAGIC)?;
    unreachable!("final-entry: shutdown returned unexpectedly");
}
