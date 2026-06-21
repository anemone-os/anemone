#![no_std]
#![no_main]
#![allow(unused)]

mod ltp;

use anemone_rs::{
    abi::{
        fs::linux::open::{O_CREAT, O_RDONLY, O_TRUNC, O_WRONLY},
        system::native::power::SHUTDOWN_MAGIC,
    },
    os::{
        anemone::{kernel_preempt::set_enabled as set_kernel_preempt_enabled, power::shutdown},
        linux::{
            fs::{chdir, chroot, close, fstatat, mkdirat, mount, openat, read, write, AtFd},
            process::{execve, fork, wait4, WStatus, WStatusRaw, WaitFor, WaitOptions},
        },
    },
    prelude::*,
};

const BOOTSTRAP_BUSYBOX_PRIMARY: &str = "/musl/busybox";
const BOOTSTRAP_BUSYBOX_FALLBACK: &str = "/glibc/busybox";

#[cfg(target_arch = "riscv64")]
const COMPETITION_DISK: &str = "/dev/vdb";

#[cfg(target_arch = "loongarch64")]
const COMPETITION_DISK: &str = "/dev/vda";

const COMP_PATH_ENV: &str = "PATH=/bin:/usr/bin:/usr/sbin:/sbin:/";
const GLIBC_TEST_SCRIPTS: &[&str] = &[
    "basic_testcode.sh",
    "lua_testcode.sh",
    "busybox_testcode.sh",
    "libctest_testcode.sh",
    // // "cyclictest_testcode.sh",
    "iozone_testcode.sh",
    // // "iperf_testcode.sh",
    "libcbench_testcode.sh",
    "lmbench_testcode.sh",
    // // "netperf_testcode.sh",
    // // "unixbench_testcode.sh",
];
const MUSL_TEST_SCRIPTS: &[&str] = &[
    "basic_testcode.sh",
    "lua_testcode.sh",
    "busybox_testcode.sh",
    "libctest_testcode.sh",
    // // "cyclictest_testcode.sh",
    "iozone_testcode.sh",
    // // "iperf_testcode.sh",
    "libcbench_testcode.sh",
    "lmbench_testcode.sh",
    // // "netperf_testcode.sh",
    // // "unixbench_testcode.sh",
];

cfg_select! {
    target_arch = "riscv64" => {
        const ACTIVE_LIB_DIR: &str = "/lib";
        const ACTIVE_LIB_DIRS: &[&str] = &["/lib"];
        const MUSL_LOADER_NAMES: &[&str] = &[
            "ld-musl-riscv64.so.1",
            "ld-musl-riscv64-sf.so.1",
        ];
        const INSTALL_BIN_SH_ASH_WRAPPER: bool = false;
        const STAGED_COMPETITION_FIXTURES: &[StagedCompetitionFixture] = &[
            StagedCompetitionFixture {
                source: "/fixtures/user-test/tools/mke2fs",
                dest: "/bin/mkfs.ext4",
            },
            StagedCompetitionFixture {
                source: "/fixtures/user-test/tools/mke2fs",
                dest: "/bin/mkfs.ext3",
            },
        ];
    },
    target_arch = "loongarch64" => {
        const ACTIVE_LIB_DIR: &str = "/lib64";
        const ACTIVE_LIB_DIRS: &[&str] = &["/lib64", "/usr/lib64"];
        const MUSL_LOADER_NAMES: &[&str] = &["ld-musl-loongarch-lp64d.so.1"];
        const INSTALL_BIN_SH_ASH_WRAPPER: bool = true;
        const STAGED_COMPETITION_FIXTURES: &[StagedCompetitionFixture] = &[
            StagedCompetitionFixture {
                source: "/fixtures/user-test/tools/mke2fs",
                dest: "/bin/mkfs.ext4",
            },
            StagedCompetitionFixture {
                source: "/fixtures/user-test/tools/mke2fs",
                dest: "/bin/mkfs.ext3",
            },
        ];
    }
}

const BIN_SH_ASH_WRAPPER: &[u8] = b"#!/bin/busybox ash\nexec /bin/busybox ash \"$@\"\n";

struct StagedCompetitionFixture {
    source: &'static str,
    dest: &'static str,
}

fn wait_child_exit_ok(pid: u32, name: &str) {
    match wait_child_status(pid, name) {
        Ok(WStatus::Exited(0)) => return,
        Ok(other) => panic!("user-test: {name} child exited unexpectedly: {other:?}"),
        Err(errno) => panic!("user-test: {name} wait4 failed: {errno:?}"),
    }
}

pub(crate) fn wait_child_status(pid: u32, name: &str) -> Result<WStatus, Errno> {
    loop {
        let mut wstatus = WStatusRaw::EMPTY;
        match wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut wstatus),
            WaitOptions::empty(),
        ) {
            Ok(Some(waited)) => {
                if waited != pid {
                    println!("user-test: {name} waited pid mismatch");
                    return Err(ECHILD);
                }
                return Ok(wstatus.read());
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
    // println!("user-test: running float test...");
    // local_run_cmd("/bin/float-test", &["float-test", "--type", "sig"], &[]);
    // println!("user-test: float test finished.");

    // 3. shm test
    // println!("user-test: running shm test...");
    // local_run_cmd("/bin/shm-test", &["shm-test"], &[]);
    // println!("user-test: shm test finished.");

    // 4. pg test
    // println!("user-test: running pg test...");
    // local_run_cmd("/bin/pg-test", &["pg-test"], &[]);
    // println!("user-test: pg test finished.");

    // 5. mmap test
    // println!("user-test: running mmap test...");
    // local_run_cmd("/bin/mmap-test", &["mmap-test"], &[]);
    // println!("user-test: mmap test finished.");

    // 6. OOM killer test
    // println!("user-test: running OOM killer test...");
    // local_run_cmd("/bin/oom-killer-test", &["oom-killer-test"], &[]);
    // println!("user-test: OOM killer test finished.");

    // 7. pthread create serial1 stress test
    // println!("user-test: running pthread create stress test...");
    // local_run_cmd(
    //     "/bin/pthread-create-stress",
    //     &["pthread-create-stress"],
    //     &[],
    // );
    // println!("user-test: pthread create stress test finished.");
}

fn ensure_dir(path: &str) {
    if fstatat(AtFd::Cwd, Path::new(path)).is_err() {
        mkdirat(AtFd::Cwd, Path::new(path), 0o755)
            .unwrap_or_else(|_| panic!("user-test: failed to create {path}"));
    }
}

fn ensure_dir_tree(path: &str) {
    let mut current = String::new();
    for component in path.split('/') {
        if component.is_empty() {
            if current.is_empty() {
                current.push('/');
            }
            continue;
        }

        if current.len() > 1 {
            current.push('/');
        }
        current.push_str(component);
        ensure_dir(current.as_str());
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

pub(crate) fn run_busybox(args: &[&str], name: &str) {
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
    // Staged tools live on the boot rootfs and disappear after chroot, so copy
    // them into the mounted competition image before entering it.
    install_staged_competition_fixtures("/mnt");

    println!("user-test: entering environment...");
    chroot("/mnt").expect("user-test: failed to chroot to /mnt");
    chdir("/").expect("user-test: failed to change directory to / after chroot");
}

fn init_competition_environment() {
    ensure_dir("/dev");
    mount(None, Path::new("/dev"), "devfs").expect("user-test: failed to mount devfs on /dev");
    mount(None, Path::new("/dev/shm"), "ramfs")
        .expect("user-test: failed to mount ramfs on /dev/shm");

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
    install_bin_sh_ash_wrapper_if_needed();

    ensure_symlink("/usr/bin", "/bin");
    ensure_symlink("/usr/sbin", "/bin");
    ensure_symlink("/sbin", "/bin");
    for loader_name in MUSL_LOADER_NAMES {
        ensure_symlink(&format!("/musl/lib/{loader_name}"), "libc.so");
    }

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

    println!("user-test: competition environment initialized.");
}

pub(crate) fn switch_runtime(family: &str) {
    println!("user-test: switching active runtime to {family}...");
    let target = format!("/{family}/lib");
    for active_lib_dir in ACTIVE_LIB_DIRS {
        replace_with_symlink(active_lib_dir, target.as_str());
    }
}

pub(crate) fn clear_tmp() {
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

fn chmod_executable_if_present(path: &str) {
    if fstatat(AtFd::Cwd, Path::new(path)).is_ok() {
        run_busybox(&["busybox", "chmod", "a+x", path], path);
    }
}

fn read_file(path: &str) -> Result<Vec<u8>, Errno> {
    let fd = openat(AtFd::Cwd, Path::new(path), O_RDONLY, 0)?;
    let mut content = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        let count = read(fd, &mut buf)?;
        if count == 0 {
            break;
        }
        content.extend_from_slice(&buf[..count]);
    }
    close(fd)?;
    Ok(content)
}

fn write_all(fd: u32, mut buf: &[u8], path: &str) {
    while !buf.is_empty() {
        let written = write(fd, buf)
            .unwrap_or_else(|errno| panic!("user-test: failed to write {path}: {errno:?}"));
        if written == 0 {
            panic!("user-test: short write while writing {path}");
        }
        buf = &buf[written..];
    }
}

fn install_bin_sh_ash_wrapper_if_needed() {
    if !INSTALL_BIN_SH_ASH_WRAPPER {
        return;
    }

    run_busybox(&["busybox", "ash", "-c", "true"], "busybox ash smoke");
    run_busybox(&["busybox", "rm", "-f", "/bin/sh"], "/bin/sh");

    let fd = openat(
        AtFd::Cwd,
        Path::new("/bin/sh"),
        O_WRONLY | O_CREAT | O_TRUNC,
        0o755,
    )
    .unwrap_or_else(|errno| panic!("user-test: failed to create /bin/sh wrapper: {errno:?}"));
    write_all(fd, BIN_SH_ASH_WRAPPER, "/bin/sh");
    close(fd)
        .unwrap_or_else(|errno| panic!("user-test: failed to close /bin/sh wrapper: {errno:?}"));

    println!("user-test: installed /bin/sh ash wrapper.");
}

fn copy_staged_fixture(source: &str, dest: &str) {
    let source_fd = openat(AtFd::Cwd, Path::new(source), O_RDONLY, 0).unwrap_or_else(|errno| {
        panic!("user-test: failed to open staged fixture source {source}: {errno:?}")
    });
    let dest_fd = openat(
        AtFd::Cwd,
        Path::new(dest),
        O_WRONLY | O_CREAT | O_TRUNC,
        0o755,
    )
    .unwrap_or_else(|errno| {
        panic!("user-test: failed to create staged fixture dest {dest}: {errno:?}")
    });

    let mut buf = [0u8; 4096];
    loop {
        let count = read(source_fd, &mut buf).unwrap_or_else(|errno| {
            panic!("user-test: failed to read staged fixture source {source}: {errno:?}")
        });
        if count == 0 {
            break;
        }
        write_all(dest_fd, &buf[..count], dest);
    }

    close(source_fd).unwrap_or_else(|errno| {
        panic!("user-test: failed to close staged fixture source {source}: {errno:?}")
    });
    close(dest_fd).unwrap_or_else(|errno| {
        panic!("user-test: failed to close staged fixture dest {dest}: {errno:?}")
    });
}

fn parent_dir(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some(("", _)) => "/",
        Some((parent, _)) => parent,
        None => ".",
    }
}

fn install_staged_competition_fixtures(mountpoint: &str) {
    for fixture in STAGED_COMPETITION_FIXTURES {
        if let Err(errno) = fstatat(AtFd::Cwd, Path::new(fixture.source)) {
            println!(
                "user-test: missing staged competition fixture: source {} -> dest {} ({errno:?})",
                fixture.source, fixture.dest
            );
            panic!("user-test: staged competition fixture source missing");
        }

        let dest = format!("{mountpoint}{}", fixture.dest);
        ensure_dir_tree(parent_dir(dest.as_str()));
        copy_staged_fixture(fixture.source, dest.as_str());
        println!(
            "user-test: installed staged competition fixture: {} -> {}",
            fixture.source, fixture.dest
        );
    }
}

fn ensure_script_entrypoint(path: &str) {
    chmod_executable_if_present(path);

    let content = read_file(path)
        .unwrap_or_else(|errno| panic!("user-test: failed to read script {path}: {errno:?}"));
    if content.starts_with(b"#!") {
        return;
    }

    let fd = openat(AtFd::Cwd, Path::new(path), O_WRONLY | O_TRUNC, 0)
        .unwrap_or_else(|errno| panic!("user-test: failed to rewrite script {path}: {errno:?}"));
    write_all(fd, b"#!/bin/sh\n", path);
    write_all(fd, &content, path);
    close(fd).unwrap_or_else(|errno| panic!("user-test: failed to close script {path}: {errno:?}"));
}

fn ensure_script_entrypoint_if_present(path: &str) {
    if fstatat(AtFd::Cwd, Path::new(path)).is_ok() {
        ensure_script_entrypoint(path);
    }
}

fn prepare_testcode(family: &str) {
    // The contest sdcard ships some helpers as bare shell command lists.  RV
    // userland happens to fall back to a shell after execve(2) returns ENOEXEC,
    // while the LA BusyBox/libc combination reports Exec format error.  Make
    // the in-guest helper entrypoints explicit so testcase scripts do not
    // depend on libc- or shell-specific ENOEXEC fallback behavior.
    for script in [
        format!("/{family}/basic/run-all.sh"),
        format!("/{family}/run-static.sh"),
        format!("/{family}/run-dynamic.sh"),
    ] {
        ensure_script_entrypoint_if_present(script.as_str());
    }
}

fn runtime_for_test_script<'a>(family: &'a str, script: &str) -> &'a str {
    // The contest images currently label basic_testcode.sh under /musl, but
    // their basic/* ELF binaries still carry the arch glibc PT_INTERP path.
    // Keep the musl script tree intact and expose the glibc loader only while
    // this script runs.  Remove this bridge once the images ship musl-linked
    // basic binaries or a loader layout that makes /musl/basic self-contained.
    if family == "musl" && script == "basic_testcode.sh" {
        "glibc"
    } else {
        family
    }
}

fn set_kernel_preempt_for_family(enabled: bool, family: &str) {
    set_kernel_preempt_enabled(enabled).unwrap_or_else(|errno| {
        panic!("user-test: failed to set kernel preemption for {family} iozone: {errno:?}")
    });
}

fn run_test_family(family: &str, scripts: &[&str]) {
    switch_runtime(family);
    prepare_testcode(family);
    clear_tmp();

    println!("user-test: running {family} competition tests...");
    let workdir = format!("/{family}");
    let mut active_runtime = family;
    for script in scripts {
        let script_path = format!("{workdir}/{script}");
        if fstatat(AtFd::Cwd, Path::new(script_path.as_str())).is_err() {
            panic!("user-test: missing competition script {script_path}");
        }
        let script_runtime = runtime_for_test_script(family, script);
        if script_runtime != active_runtime {
            println!("user-test: using {script_runtime} runtime for {family} {script}...");
            switch_runtime(script_runtime);
            active_runtime = script_runtime;
        }
        println!("user-test: running {family} {script}...");
        let is_iozone = *script == "iozone_testcode.sh";
        if is_iozone {
            // println!("user-test: disabling kernel preemption for {family}
            // iozone..."); set_kernel_preempt_for_family(false,
            // family);
        }
        run_busybox_in_dir(workdir.as_str(), &["busybox", "sh", script], script);
        if is_iozone {
            // set_kernel_preempt_for_family(true, family);
            // println!("user-test: re-enabled kernel preemption after {family}
            // iozone.");
        }
    }
    println!("user-test: {family} competition tests finished.");
}

/// competition tests.
fn run_comp_tests() {
    mount_competition_root();
    init_competition_environment();
    ltp::install_ltp_fixtures();
    run_busybox(
        &[
            "busybox",
            "chmod",
            "a+x",
            "/glibc/basic/run-all.sh",
            "/musl/basic/run-all.sh",
        ],
        "basic run-all chmod",
    );

    run_test_family("glibc", GLIBC_TEST_SCRIPTS);
    run_test_family("musl", MUSL_TEST_SCRIPTS);
    ltp::run_ltp_tests();

    println!("user-test: all competition tests finished.");
}

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    run_local_tests();

    run_comp_tests();

    println!("user-test: all tests finished, shutting down.");
    shutdown(SHUTDOWN_MAGIC).expect("user-test: failed to request shutdown");
    unreachable!("user-test: shutdown returned unexpectedly");
}
