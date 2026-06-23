use anemone_rs::{
    os::linux::{
        fs::chdir,
        process::{WStatus, WStatusRaw, WaitFor, WaitOptions, execve, fork, wait4},
    },
    prelude::*,
};

fn wait_child_exit_ok(pid: u32, name: &str) {
    match wait_child_status(pid, name) {
        Ok(WStatus::Exited(0)) => return,
        Ok(other) => panic!("user-test: {name} child exited unexpectedly: {other:?}"),
        Err(errno) => panic!("user-test: {name} wait4 failed: {errno:?}"),
    }
}

fn wait_child_status(pid: u32, name: &str) -> Result<WStatus, Errno> {
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

pub(crate) fn run_execve_in_dir(
    workdir: Option<&str>,
    cmd: &str,
    args: &[&str],
    envs: &[&str],
    name: &str,
) {
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

pub(crate) fn run_execve(cmd: &str, args: &[&str], envs: &[&str], name: &str) {
    run_execve_in_dir(None, cmd, args, envs, name);
}
