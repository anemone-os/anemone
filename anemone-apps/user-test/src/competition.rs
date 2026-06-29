use anemone_rs::{
    abi::fs::linux::open::{O_RDONLY, O_TRUNC, O_WRONLY},
    os::linux::fs::{close, fstatat, openat, read, AtFd},
    prelude::*,
};

const GLIBC_TEST_SCRIPTS: &[&str] = &[
    // "basic_testcode.sh",
    // "lua_testcode.sh",
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

pub(crate) fn run_competition_tests() {
    run_test_family("glibc", GLIBC_TEST_SCRIPTS);
    run_test_family("musl", MUSL_TEST_SCRIPTS);
}

fn chmod_executable_if_present(path: &str) {
    if fstatat(AtFd::Cwd, Path::new(path)).is_ok() {
        crate::busybox::run_busybox(&["busybox", "chmod", "a+x", path], path);
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

fn ensure_script_entrypoint(path: &str) {
    chmod_executable_if_present(path);

    let content = read_file(path)
        .unwrap_or_else(|errno| panic!("user-test: failed to read script {path}: {errno:?}"));
    if content.starts_with(b"#!") {
        return;
    }

    let fd = openat(AtFd::Cwd, Path::new(path), O_WRONLY | O_TRUNC, 0)
        .unwrap_or_else(|errno| panic!("user-test: failed to rewrite script {path}: {errno:?}"));
    crate::file::write_all(fd, b"#!/bin/sh\n", path);
    crate::file::write_all(fd, &content, path);
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

fn run_test_family(family: &str, scripts: &[&str]) {
    crate::runtime::switch_runtime(family);
    prepare_testcode(family);
    crate::runtime::clear_tmp();

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
            crate::runtime::switch_runtime(script_runtime);
            active_runtime = script_runtime;
        }
        println!("user-test: running {family} {script}...");
        crate::busybox::run_busybox_in_dir(workdir.as_str(), &["busybox", "sh", script], script);
    }
    println!("user-test: {family} competition tests finished.");
}
