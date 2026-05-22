#![no_std]
#![no_main]
#![allow(unused)]

mod ltp;

use anemone_rs::{
    os::linux::{
        fs::{chdir, chroot, fstatat, mkdirat, mount, AtFd},
        process::{execve, fork, wait4, WStatus, WStatusRaw, WaitFor, WaitOptions},
    },
    prelude::*,
};

const BOOTSTRAP_BUSYBOX_PRIMARY: &str = "/musl/busybox";
const BOOTSTRAP_BUSYBOX_FALLBACK: &str = "/glibc/busybox";
const COMPETITION_DISK: &str = "/dev/vdb";
const COMP_PATH_ENV: &str = "PATH=/bin:/usr/bin:/usr/sbin:/sbin:/";
const GLIBC_TEST_SCRIPTS: &[&str] = &[
    // "basic_testcode.sh",
    "lua_testcode.sh",
    // "busybox_testcode.sh",
    // "libctest_testcode.sh",
    // "cyclictest_testcode.sh",
    // "iozone_testcode.sh",
    // "iperf_testcode.sh",
    // "libcbench_testcode.sh",
    // "lmbench_testcode.sh",
    // "netperf_testcode.sh",
    // "unixbench_testcode.sh",
];
const MUSL_TEST_SCRIPTS: &[&str] = &[
    // "basic_testcode.sh",
    // "lua_testcode.sh",
    // "busybox_testcode.sh",
    "libctest_testcode.sh",
    // "cyclictest_testcode.sh",
    // "iozone_testcode.sh",
    // "iperf_testcode.sh",
    // "libcbench_testcode.sh",
    // "lmbench_testcode.sh",
    // "netperf_testcode.sh",
    // "unixbench_testcode.sh",
];

cfg_select! {
    target_arch = "riscv64" => {
        const ACTIVE_LIB_DIR: &str = "/lib";
        const MUSL_LOADER_NAME: &str = "ld-musl-riscv64-sf.so.1";
    },
    target_arch = "loongarch64" => {
        const ACTIVE_LIB_DIR: &str = "/lib64";
        const MUSL_LOADER_NAME: &str = "ld-musl-loongarch-lp64d.so.1";
    }
}

fn wait_child_exit_ok(pid: u32, name: &str) {
    loop {
        let mut wstatus = WStatusRaw::EMPTY;
        match wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut wstatus),
            WaitOptions::empty(),
        ) {
            Ok(Some(waited)) => {
                assert_eq!(waited, pid, "user-test: {name} waited pid mismatch");
                match wstatus.read() {
                    WStatus::Exited(0) => return,
                    other => panic!("user-test: {name} child exited unexpectedly: {other:?}"),
                }
            },
            Ok(None) => {
                panic!("user-test: {name} wait4 returned None without WNOHANG");
            },
            Err(EINTR) => continue,
            Err(errno) => panic!("user-test: {name} wait4 failed: {errno:?}"),
        }
    }
}

fn run_execve_in_dir(workdir: Option<&str>, cmd: &str, args: &[&str], envs: &[&str], name: &str) {
    match fork().expect("user-test: failed to fork") {
        Some(tid) => {
            wait_child_exit_ok(tid, name);
        },
        None => {
            if let Some(dir) = workdir {
                chdir(dir).expect("user-test: failed to chdir before execve");
            }
            execve(cmd, args, envs).expect("user-test: failed to execve");
        },
    }
}

fn run_execve(cmd: &str, args: &[&str], envs: &[&str], name: &str) {
    run_execve_in_dir(None, cmd, args, envs, name);
}

fn local_run_cmd(cmd: &str, args: &[&str], envs: &[&str]) {
    run_execve(cmd, args, envs, cmd);
}

/// local tests for development.
fn run_local_tests() {
    // 1. signal test
    // println!("user-test: running signal test...");
    // local_run_cmd("/bin/signal-test", &["signal-test"], &[]);
    // println!("user-test: signal test finished.");

    // 2. float test
    println!("user-test: running float test...");
    local_run_cmd("/bin/float-test", &["float-test", "--type", "sig"], &[]);
    println!("user-test: float test finished.");
}

fn ensure_dir(path: &str) {
    if fstatat(AtFd::Cwd, Path::new(path)).is_err() {
        mkdirat(AtFd::Cwd, Path::new(path), 0o755)
            .unwrap_or_else(|_| panic!("user-test: failed to create {path}"));
    }
}

fn bootstrap_busybox() -> &'static str {
    if fstatat(AtFd::Cwd, Path::new(BOOTSTRAP_BUSYBOX_PRIMARY)).is_ok() {
        BOOTSTRAP_BUSYBOX_PRIMARY
    } else if fstatat(AtFd::Cwd, Path::new(BOOTSTRAP_BUSYBOX_FALLBACK)).is_ok() {
        BOOTSTRAP_BUSYBOX_FALLBACK
    } else {
        panic!("user-test: no static busybox found under /musl or /glibc");
    }
}

fn run_comp_exec(cmd: &str, args: &[&str], name: &str) {
    let ld_library_path = format!("LD_LIBRARY_PATH={ACTIVE_LIB_DIR}");
    let envs = [COMP_PATH_ENV, ld_library_path.as_str()];
    run_execve(cmd, args, &envs, name);
}

fn run_comp_exec_in_dir(workdir: &str, cmd: &str, args: &[&str], name: &str) {
    let ld_library_path = format!("LD_LIBRARY_PATH={ACTIVE_LIB_DIR}");
    let envs = [COMP_PATH_ENV, ld_library_path.as_str()];
    run_execve_in_dir(Some(workdir), cmd, args, &envs, name);
}

fn run_bootstrap_busybox(args: &[&str], name: &str) {
    run_comp_exec(bootstrap_busybox(), args, name);
}

fn run_busybox(args: &[&str], name: &str) {
    run_comp_exec("/bin/busybox", args, name);
}

fn run_busybox_in_dir(workdir: &str, args: &[&str], name: &str) {
    run_comp_exec_in_dir(workdir, "/bin/busybox", args, name);
}

fn ensure_symlink(link_path: &str, target: &str) {
    if fstatat(AtFd::Cwd, Path::new(link_path)).is_err() {
        run_busybox(&["busybox", "ln", "-s", target, link_path], link_path);
    }
}

fn replace_with_symlink(link_path: &str, target: &str) {
    run_busybox(&["busybox", "rm", "-rf", link_path], link_path);
    run_busybox(&["busybox", "ln", "-s", target, link_path], link_path);
}

fn mount_competition_root() {
    mount(None, Path::new("/dev"), "devfs").expect("user-test: failed to mount devfs on /dev");
    mount(Some(Path::new(COMPETITION_DISK)), Path::new("/mnt"), "ext4")
        .expect("user-test: failed to mount /dev/vdb on /mnt with ext4");

    chroot("/mnt").expect("user-test: failed to chroot to /mnt");
    chdir("/").expect("user-test: failed to change directory to / after chroot");
}

fn init_competition_environment() {
    ensure_dir("/dev");
    mount(None, Path::new("/dev"), "devfs").expect("user-test: failed to mount devfs on /dev");

    ensure_dir("/tmp");
    mount(None, Path::new("/tmp"), "ramfs").expect("user-test: failed to mount ramfs on /tmp");

    ensure_dir("/proc");
    mount(None, Path::new("/proc"), "procfs").expect("user-test: failed to mount procfs on /proc");

    ensure_dir("/bin");
    ensure_dir("/usr");

    run_bootstrap_busybox(&["busybox", "rm", "-f", "/bin/busybox"], "/bin/busybox");
    run_bootstrap_busybox(
        &["busybox", "ln", "-s", bootstrap_busybox(), "/bin/busybox"],
        "/bin/busybox",
    );
    run_busybox(&["busybox", "--install", "-s", "/bin"], "busybox --install");

    ensure_symlink("/usr/bin", "/bin");
    ensure_symlink("/usr/sbin", "/bin");
    ensure_symlink("/sbin", "/bin");
    ensure_symlink(&format!("/musl/lib/{MUSL_LOADER_NAME}"), "libc.so");

    if fstatat(AtFd::Cwd, Path::new("/glibc/lib/libc.so")).is_ok()
        && fstatat(AtFd::Cwd, Path::new("/glibc/lib/libc.so.6")).is_err()
    {
        ensure_symlink("/glibc/lib/libc.so.6", "libc.so");
    }

    if fstatat(AtFd::Cwd, Path::new("/glibc/lib/libm.so")).is_ok()
        && fstatat(AtFd::Cwd, Path::new("/glibc/lib/libm.so.6")).is_err()
    {
        ensure_symlink("/glibc/lib/libm.so.6", "libm.so");
    }

    println!("testing procfs...");
    run_busybox(&["busybox", "ls", "/proc"], "ls /proc");
    println!("user-test: competition environment initialized.");
}

fn switch_runtime(family: &str) {
    println!("user-test: switching active runtime to {family}...");
    let target = format!("/{family}/lib");
    replace_with_symlink(ACTIVE_LIB_DIR, target.as_str());
}

fn clear_tmp() {
    run_busybox(
        &[
            "busybox",
            "find",
            "/tmp",
            "-mindepth",
            "1",
            "-maxdepth",
            "1",
            "-exec",
            "/bin/busybox",
            "rm",
            "-rf",
            "{}",
            ";",
        ],
        "clear /tmp",
    );
}

fn run_test_family(family: &str, scripts: &[&str]) {
    switch_runtime(family);
    clear_tmp();

    println!("user-test: running {family} competition tests...");
    let workdir = format!("/{family}");
    for script in scripts {
        let script_path = format!("{workdir}/{script}");
        if fstatat(AtFd::Cwd, Path::new(script_path.as_str())).is_err() {
            panic!("user-test: missing competition script {script_path}");
        }
        println!("user-test: running {family} {script}...");
        run_busybox_in_dir(workdir.as_str(), &["busybox", "sh", script], script);
    }
    println!("user-test: {family} competition tests finished.");
}

/// competition tests.
fn run_comp_tests() {
    mount_competition_root();
    init_competition_environment();

    run_test_family("glibc", GLIBC_TEST_SCRIPTS);
    run_test_family("musl", MUSL_TEST_SCRIPTS);

    println!("user-test: all competition tests finished.");
}

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    run_local_tests();

    run_comp_tests();

    loop {}
}
