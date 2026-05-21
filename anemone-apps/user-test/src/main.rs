#![no_std]
#![no_main]
#![allow(unused)]

use anemone_rs::{
    os::linux::{
        fs::{chdir, chroot, fstatat, mkdirat, mount, AtFd},
        process::{execve, fork, wait4, WStatusRaw, WaitFor, WaitOptions},
    },
    prelude::*,
};

fn local_run_cmd(cmd: &str, args: &[&str], envs: &[&str]) {
    match fork().expect("user-test: failed to fork") {
        Some(tid) => {
            // parent
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
            // child
            execve(cmd, args, envs).expect("user-test: failed to execve");
        },
    }
}

/// local tests for development.
fn run_local_tests() {
    // 1. signal test
    println!("user-test: running signal test...");
    local_run_cmd("/bin/signal-test", &["signal-test"], &[]);
    println!("user-test: signal test finished.");
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

    let stat = fstatat(AtFd::Cwd, Path::new("/proc"));
    if stat.is_err() {
        mkdirat(AtFd::Cwd, Path::new("/proc"), 0o755)
            .expect("user-test: failed to create /proc directory");
    }
    mount(None, Path::new("/proc"), "procfs").expect("user-test: failed to mount procfs on /proc");

    // TODO: sysfs, etc.

    // install busybox
    let stat = fstatat(AtFd::Cwd, Path::new("/bin"));
    if stat.is_err() {
        mkdirat(AtFd::Cwd, Path::new("/bin"), 0o755)
            .expect("user-test: failed to create /bin directory");

        comp_run_cmd("/glibc/busybox --install -s /bin");
    }

    // cp lib to /
    let stat = fstatat(AtFd::Cwd, Path::new("/lib"));
    if stat.is_err() {
        mkdirat(AtFd::Cwd, Path::new("/lib"), 0o755)
            .expect("user-test: failed to create /lib directory");

        comp_run_cmd("/bin/cp -r /musl/lib /");

        // musl's libc.so is a dynamic linker as well. we should create a symlink for
        // it. for now we just cp.
        comp_run_cmd("/bin/cp /musl/lib/libc.so /lib/ld-musl-riscv64-sf.so.1");
    }

    // cp busybox to /
    let stat = fstatat(AtFd::Cwd, Path::new("/busybox"));
    if stat.is_err() {
        println!("user-test: copying busybox to /");
        comp_run_cmd("/bin/cp /glibc/busybox /");
    }

    // cp busybox to /bin
    let stat = fstatat(AtFd::Cwd, Path::new("/bin/busybox"));
    if stat.is_err() {
        println!("user-test: copying busybox to /bin");
        comp_run_cmd("/bin/cp /glibc/busybox /bin");
    }

    // test procfs
    println!("testing procfs...");
    comp_run_cmd("/bin/ls /proc");

    // done.
    println!("user-test: environment initialized.");
}

fn comp_run_cmd(cmd: &str) {
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

/// competition tests.
fn run_comp_tests() {
    init_environment();

    // 1. basic tests
    // println!("user-test: running basic tests...");
    // chdir("/glibc").expect("user-test: failed to change directory to
    // /glibc"); comp_run_cmd("./basic_testcode.sh");
    // println!("user-test: basic tests passed.");

    // 2. lua tests
    // println!("user-test: running lua tests...");
    // chdir("/glibc").expect("user-test: failed to change directory to
    // /glibc"); comp_run_cmd("./lua_testcode.sh");
    // println!("user-test: lua tests passed.");

    // 3. busybox tests
    // println!("user-test: running busybox tests...");
    // chdir("/glibc").expect("user-test: failed to change directory to
    // /glibc"); comp_run_cmd("./busybox_testcode.sh");
    // println!("user-test: busybox tests passed.");

    // 4. static-linked libc tests
    println!("user-test: running static-linked libc tests...");
    chdir("/musl").expect("user-test: failed to change directory to /musl");
    comp_run_cmd("./run-static.sh");
    println!("user-test: static-linked libc tests passed.");

    // 5. dynamic-linked libc tests
    // println!("user-test: running dynamic-linked libc tests...");
    // chdir("/musl").expect("user-test: failed to change directory to /musl");
    // comp_run_cmd("./run-dynamic.sh");
    // println!("user-test: dynamic-linked libc tests passed.");

    // 6. lmbench tests
    // println!("user-test: running lmbench tests...");
    // chdir("/glibc").expect("user-test: failed to change directory to
    // /glibc"); comp_run_cmd("./lmbench_testcode.sh");
    // println!("user-test: lmbench tests passed.");
}

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    // run_local_tests();

    run_comp_tests();

    loop {}
}
