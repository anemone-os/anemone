use anemone_rs::{
    abi::fs::linux::open::{O_RDONLY, O_TRUNC, O_WRONLY},
    os::linux::{
        fs::{AtFd, close, openat, read},
        process::WStatus,
    },
    prelude::*,
};

const GLIBC_SCRIPTS: &[&str] = &[
    "basic_testcode.sh",
    "lua_testcode.sh",
    "busybox_testcode.sh",
    "libctest_testcode.sh",
    "cyclictest_testcode.sh",
    "iozone_testcode.sh",
    "iperf_testcode.sh",
    "libcbench_testcode.sh",
    "lmbench_testcode.sh",
    "netperf_testcode.sh",
    "unixbench_testcode.sh",
];
const MUSL_SCRIPTS: &[&str] = GLIBC_SCRIPTS;

fn prepare_environment() -> Result<(), Errno> {
    for path in ["/bin", "/usr"] {
        crate::environment::ensure_dir(path, 0o755)?;
    }

    crate::busybox::run_bootstrap_busybox(&["busybox", "rm", "-f", "/bin/busybox"], "/bin/busybox");
    crate::busybox::run_bootstrap_busybox(
        &[
            "busybox",
            "ln",
            "-s",
            crate::busybox::bootstrap_busybox(),
            "/bin/busybox",
        ],
        "/bin/busybox",
    );
    crate::busybox::run_busybox(&["busybox", "--install", "-s", "/bin"], "busybox --install");
    crate::runtime::install_bin_sh_ash_wrapper_if_needed();

    crate::busybox::ensure_symlink("/usr/bin", "/bin");
    crate::busybox::ensure_symlink("/usr/sbin", "/bin");
    crate::busybox::ensure_symlink("/sbin", "/bin");
    crate::runtime::ensure_runtime_loader_links();

    crate::embedded_tools::install()?;
    crate::ltp::install_ltp_fixtures();
    println!("final-submit: preliminary environment initialized");
    Ok(())
}

fn chmod_executable_if_present(path: &str) {
    if crate::environment::path_exists(path) {
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
        .unwrap_or_else(|errno| panic!("final-submit: failed to read script {path}: {errno:?}"));
    if content.starts_with(b"#!") {
        return;
    }

    let fd = openat(AtFd::Cwd, Path::new(path), O_WRONLY | O_TRUNC, 0)
        .unwrap_or_else(|errno| panic!("final-submit: failed to rewrite script {path}: {errno:?}"));
    crate::file::write_all(fd, b"#!/bin/sh\n", path);
    crate::file::write_all(fd, &content, path);
    close(fd)
        .unwrap_or_else(|errno| panic!("final-submit: failed to close script {path}: {errno:?}"));
}

fn prepare_testcode(family: &str) {
    // Preserve the same frozen-image compatibility used by `user-test`: some
    // helper scripts lack an execute bit or shebang on one architecture.
    for script in [
        format!("/{family}/test.sh"),
        format!("/{family}/basic/run-all.sh"),
        format!("/{family}/run-static.sh"),
        format!("/{family}/run-dynamic.sh"),
    ] {
        if crate::environment::path_exists(script.as_str()) {
            ensure_script_entrypoint(script.as_str());
        }
    }
}

fn runtime_for_test_script<'a>(family: &'a str, script: &str) -> &'a str {
    // The frozen preliminary images label basic_testcode.sh under /musl while
    // its binaries still use the glibc interpreter.
    if family == "musl" && script == "basic_testcode.sh" {
        "glibc"
    } else {
        family
    }
}

fn run_test_family(family: &str, scripts: &[&str]) {
    crate::runtime::switch_runtime(family);
    crate::runtime::clear_tmp();
    prepare_testcode(family);

    let workdir = format!("/{family}");
    let mut active_runtime = family;
    for script in scripts {
        let path = format!("{workdir}/{script}");
        if !crate::environment::path_exists(path.as_str()) {
            eprintln!("final-submit: skipping missing preliminary script {path}");
            continue;
        }

        let script_runtime = runtime_for_test_script(family, script);
        if script_runtime != active_runtime {
            crate::runtime::switch_runtime(script_runtime);
            active_runtime = script_runtime;
        }

        println!("final-submit: starting preliminary {family} script {script}");
        let ld_library_path = format!("LD_LIBRARY_PATH={}", crate::runtime::active_lib_dir());
        let envs = [
            "PATH=/bin:/usr/bin:/usr/sbin:/sbin:/",
            ld_library_path.as_str(),
        ];
        match crate::process::run_status_in_dir(
            Some(workdir.as_str()),
            "/bin/busybox",
            &["busybox", "sh", script],
            &envs,
            script,
        ) {
            Ok(WStatus::Exited(0)) => {
                println!("final-submit: preliminary {family} script {script} passed")
            },
            Ok(status) => eprintln!(
                "final-submit: preliminary {family} script {script} finished with {status:?}",
            ),
            Err(errno) => {
                eprintln!("final-submit: preliminary {family} script {script} failed: {errno}")
            },
        }
    }
}

pub(crate) fn run() -> Result<(), Errno> {
    prepare_environment()?;

    // LTP carries the largest preliminary score. Run the frozen submission
    // whitelist before the longer benchmark-oriented script groups.
    crate::ltp::run_ltp_tests();
    run_test_family("glibc", GLIBC_SCRIPTS);
    run_test_family("musl", MUSL_SCRIPTS);
    println!("final-submit: all preliminary workloads finished");
    Ok(())
}
