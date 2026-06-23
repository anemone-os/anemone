use anemone_rs::{
    os::linux::fs::{AtFd, fstatat},
    prelude::*,
};

const BOOTSTRAP_BUSYBOX_PRIMARY: &str = "/musl/busybox";
const BOOTSTRAP_BUSYBOX_FALLBACK: &str = "/glibc/busybox";
const COMP_PATH_ENV: &str = "PATH=/bin:/usr/bin:/usr/sbin:/sbin:/";

pub(crate) fn bootstrap_busybox() -> &'static str {
    if fstatat(AtFd::Cwd, Path::new(BOOTSTRAP_BUSYBOX_PRIMARY)).is_ok() {
        BOOTSTRAP_BUSYBOX_PRIMARY
    } else if fstatat(AtFd::Cwd, Path::new(BOOTSTRAP_BUSYBOX_FALLBACK)).is_ok() {
        BOOTSTRAP_BUSYBOX_FALLBACK
    } else {
        panic!("user-test: no static busybox found under /musl or /glibc");
    }
}

fn run_comp_exec(cmd: &str, args: &[&str], name: &str) {
    let ld_library_path = format!("LD_LIBRARY_PATH={}", crate::runtime::active_lib_dir());
    let envs = [COMP_PATH_ENV, ld_library_path.as_str()];
    crate::process::run_execve(cmd, args, &envs, name);
}

fn run_comp_exec_in_dir(workdir: &str, cmd: &str, args: &[&str], name: &str) {
    let ld_library_path = format!("LD_LIBRARY_PATH={}", crate::runtime::active_lib_dir());
    let envs = [COMP_PATH_ENV, ld_library_path.as_str()];
    crate::process::run_execve_in_dir(Some(workdir), cmd, args, &envs, name);
}

pub(crate) fn run_bootstrap_busybox(args: &[&str], name: &str) {
    run_comp_exec(bootstrap_busybox(), args, name);
}

pub(crate) fn run_busybox(args: &[&str], name: &str) {
    run_comp_exec("/bin/busybox", args, name);
}

pub(crate) fn run_busybox_in_dir(workdir: &str, args: &[&str], name: &str) {
    run_comp_exec_in_dir(workdir, "/bin/busybox", args, name);
}

pub(crate) fn ensure_symlink(link_path: &str, target: &str) {
    if fstatat(AtFd::Cwd, Path::new(link_path)).is_err() {
        run_busybox(&["busybox", "ln", "-s", target, link_path], link_path);
    }
}

pub(crate) fn replace_with_symlink(link_path: &str, target: &str) {
    run_busybox(&["busybox", "rm", "-rf", link_path], link_path);
    run_busybox(&["busybox", "ln", "-s", target, link_path], link_path);
}
