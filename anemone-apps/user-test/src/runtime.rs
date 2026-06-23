use anemone_rs::{
    abi::fs::linux::open::{O_CREAT, O_TRUNC, O_WRONLY},
    os::linux::fs::{AtFd, close, fstatat, openat},
    prelude::*,
};

cfg_select! {
    target_arch = "riscv64" => {
        const ACTIVE_LIB_DIR: &str = "/lib";
        const ACTIVE_LIB_DIRS: &[&str] = &["/lib"];
        const MUSL_LOADER_NAMES: &[&str] = &[
            "ld-musl-riscv64.so.1",
            "ld-musl-riscv64-sf.so.1",
        ];
        const INSTALL_BIN_SH_ASH_WRAPPER: bool = false;
    },
    target_arch = "loongarch64" => {
        const ACTIVE_LIB_DIR: &str = "/lib64";
        const ACTIVE_LIB_DIRS: &[&str] = &["/lib64", "/usr/lib64"];
        const MUSL_LOADER_NAMES: &[&str] = &["ld-musl-loongarch-lp64d.so.1"];
        const INSTALL_BIN_SH_ASH_WRAPPER: bool = true;
    }
}

const BIN_SH_ASH_WRAPPER: &[u8] = b"#!/bin/busybox ash\nexec /bin/busybox ash \"$@\"\n";

pub(crate) fn active_lib_dir() -> &'static str {
    ACTIVE_LIB_DIR
}

pub(crate) fn switch_runtime(family: &str) {
    println!("user-test: switching active runtime to {family}...");
    let target = format!("/{family}/lib");
    for active_lib_dir in ACTIVE_LIB_DIRS {
        crate::busybox::replace_with_symlink(active_lib_dir, target.as_str());
    }
}

pub(crate) fn clear_tmp() {
    crate::busybox::run_busybox(
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

pub(crate) fn install_bin_sh_ash_wrapper_if_needed() {
    if !INSTALL_BIN_SH_ASH_WRAPPER {
        return;
    }

    crate::busybox::run_busybox(&["busybox", "ash", "-c", "true"], "busybox ash smoke");
    crate::busybox::run_busybox(&["busybox", "rm", "-f", "/bin/sh"], "/bin/sh");

    let fd = openat(
        AtFd::Cwd,
        Path::new("/bin/sh"),
        O_WRONLY | O_CREAT | O_TRUNC,
        0o755,
    )
    .unwrap_or_else(|errno| panic!("user-test: failed to create /bin/sh wrapper: {errno:?}"));
    crate::file::write_all(fd, BIN_SH_ASH_WRAPPER, "/bin/sh");
    close(fd)
        .unwrap_or_else(|errno| panic!("user-test: failed to close /bin/sh wrapper: {errno:?}"));

    println!("user-test: installed /bin/sh ash wrapper.");
}

pub(crate) fn ensure_runtime_loader_links() {
    for loader_name in MUSL_LOADER_NAMES {
        crate::busybox::ensure_symlink(&format!("/musl/lib/{loader_name}"), "libc.so");
    }

    if fstatat(AtFd::Cwd, Path::new("/glibc/lib/libc.so")).is_ok()
        && fstatat(AtFd::Cwd, Path::new("/glibc/lib/libc.so.6")).is_err()
    {
        crate::busybox::ensure_symlink("/glibc/lib/libc.so.6", "libc.so");
    }

    if fstatat(AtFd::Cwd, Path::new("/glibc/lib/libm.so")).is_ok()
        && fstatat(AtFd::Cwd, Path::new("/glibc/lib/libm.so.6")).is_err()
    {
        crate::busybox::ensure_symlink("/glibc/lib/libm.so.6", "libm.so");
    }
}
