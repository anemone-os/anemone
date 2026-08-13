use anemone_rs::{
    os::linux::{
        fs::chdir,
        process::{WStatus, WStatusRaw, WaitFor, WaitOptions, execve, exit, fork, wait4},
    },
    prelude::*,
};

fn wait_child_status(pid: u32, name: &str) -> Result<WStatus, Errno> {
    loop {
        let mut status = WStatusRaw::EMPTY;
        match wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut status),
            WaitOptions::empty(),
        ) {
            Ok(Some(waited)) if waited == pid => return Ok(status.read()),
            Ok(Some(_)) => {
                eprintln!("final-submit: {name} waited pid mismatch");
                return Err(ECHILD);
            },
            Ok(None) => unreachable!("blocking wait4 returned no child"),
            Err(EINTR) => {},
            Err(errno) => return Err(errno),
        }
    }
}

pub(crate) fn run_status_in_dir(
    workdir: Option<&str>,
    cmd: &str,
    args: &[&str],
    envs: &[&str],
    name: &str,
) -> Result<WStatus, Errno> {
    match fork()? {
        Some(child) => wait_child_status(child, name),
        None => {
            if let Some(dir) = workdir {
                if let Err(errno) = chdir(dir) {
                    eprintln!("final-submit: {name} chdir({dir}) failed: {errno}");
                    exit(127);
                }
            }
            if let Err(errno) = execve(cmd, args, envs) {
                eprintln!("final-submit: {name} execve({cmd}) failed: {errno}");
            }
            exit(127);
        },
    }
}

pub(crate) fn run_execve_in_dir(
    workdir: Option<&str>,
    cmd: &str,
    args: &[&str],
    envs: &[&str],
    name: &str,
) {
    match run_status_in_dir(workdir, cmd, args, envs, name) {
        Ok(WStatus::Exited(0)) => {},
        Ok(status) => panic!("final-submit: {name} child exited unexpectedly: {status:?}"),
        Err(errno) => panic!("final-submit: {name} wait failed: {errno:?}"),
    }
}

pub(crate) fn run_execve(cmd: &str, args: &[&str], envs: &[&str], name: &str) {
    run_execve_in_dir(None, cmd, args, envs, name);
}
