#![no_std]
#![no_main]
#![warn(unused)]

use anemone_rs::{
    os::linux::{
        fs::{chdir, chroot, fstatat, mkdirat, mount, AtFd},
        process::{execve, fork, wait4, WStatusRaw, WaitFor, WaitOptions},
    },
    prelude::*,
};
fn run_cmd(cmd: &str) {
    match fork().expect("user-test: failed to fork") {
        Some(tid) => {
            // parent. wait for child to exit.
            let mut wstatus = WStatusRaw::EMPTY;
            match wait4(
                WaitFor::ChildWithTgid(tid),
                Some(&mut wstatus),
                WaitOptions::empty(),
            )
            .expect("user-test: failed to wait4")
            {
                Some(tid) => {
                    println!(
                        "user-test: child task #{} exited with code {:?}",
                        tid,
                        wstatus.read()
                    )
                },
                None => {
                    panic!("user-test: wait4 returned None unexpectedly");
                },
            }
        },
        None => {
            execve(
                "/glibc/busybox",
                &["busybox", "sh", "-c", cmd],
                &["PATH=/:/bin:/lib", "LD_LIBRARY_PATH=/lib"],
            )
            .expect("user-test: failed to execve");
        },
    }
}

fn init_environment() {
    mount(None, Path::new("/dev"), "devfs").expect("user-test: failed to mount devfs on /dev");
    mount(Some(Path::new("/dev/vdb")), Path::new("/mnt"), "ext4")
        .expect("user-test: failed to mount /dev/vdb on /mnt with ext4");

    chroot("/mnt").expect("user-test: failed to chroot to /mnt");
    chdir("/").expect("user-test: failed to change directory to / after chroot");

    // now we should mount devfs again, for this new root.
    let stat = fstatat(AtFd::Cwd, Path::new("dev"));
    if stat.is_err() {
        mkdirat(AtFd::Cwd, Path::new("dev"), 0o755)
            .expect("user-test: failed to create /dev directory");
    }
    mount(None, Path::new("/dev"), "devfs").expect("user-test: failed to mount devfs on /dev");

    let stat = fstatat(AtFd::Cwd, Path::new("tmp"));
    if stat.is_err() {
        mkdirat(AtFd::Cwd, Path::new("tmp"), 0o755)
            .expect("user-test: failed to create /tmp directory");
    }
    mount(None, Path::new("/tmp"), "ramfs").expect("user-test: failed to mount ramfs on /tmp");

    // TODO: procfs, sysfs, etc.

    // install busybox
    let stat = fstatat(AtFd::Cwd, Path::new("/bin"));
    if stat.is_err() {
        mkdirat(AtFd::Cwd, Path::new("/bin"), 0o755)
            .expect("user-test: failed to create /bin directory");

        run_cmd("/glibc/busybox --install -s /bin");
    }

    // cp lib to /
    let stat = fstatat(AtFd::Cwd, Path::new("/lib"));
    if stat.is_err() {
        mkdirat(AtFd::Cwd, Path::new("/lib"), 0o755)
            .expect("user-test: failed to create /lib directory");

        run_cmd("/bin/cp -r /glibc/lib /");
    }

    // done.
}

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    init_environment();

    println!("user-test: environment initialized.");

    // 1. basic tests
    println!("user-test: running basic tests...");
    chdir("/glibc/basic").expect("user-test: failed to change directory to /glibc/basic");
    run_cmd("./run-all.sh");
    chdir("..").expect("user-test: failed to change directory to /glibc after basic tests");
    println!("user-test: basic tests passed.");

    loop {}
}
