#![no_std]
#![no_main]

use core::ptr::null_mut;

use anemone_rs::{
    abi::process::linux::signal::SIGCHLD,
    env::*,
    os::linux::process::{
        clone, execve, sched_yield, wait4, CloneFlags, WStatusRaw, WaitFor, WaitOptions,
    },
    prelude::*,
    process::process_id,
};

const USER_TEST_PATH: &str = "/bin/user-test";
const USER_TEST_ARGV: &[&str] = &["user-test"];
const USER_TEST_ENVP: &[&str] = &[];

fn log_execve_payload(path: &str, argv: &[&str], envp: &[&str]) {
    println!("init: execve payload:");
    println!("init:   path={path:?}");
    println!("init:   argc={}", argv.len());
    for (index, arg) in argv.iter().enumerate() {
        println!("init:   argv[{index}]={arg:?}");
    }
    println!("init:   envc={}", envp.len());
    for (index, env) in envp.iter().enumerate() {
        println!("init:   envp[{index}]={env:?}");
    }
    if envp.is_empty() {
        println!("init:   envp is empty");
    }
}

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    let cwd = current_dir()?;
    let pid = process_id();
    println!("init: started:\n\tcwd:{}\n\tpid:{}", cwd.display(), pid);
    let env = envs();
    for (key, value) in env {
        println!("init: env {}={}", key, value);
    }

    // auxv test
    {
        println!("page size: {:#x?}", page_sz());
        println!("random bytes: {:#x?}", random_bytes());
        println!("clock ticks per second: {:#x?}", clktck());
        println!("exec filename: {:#x?}", exec_fn());
        println!("platform: {:#x?}", platform());
        println!("base platform: {:#x?}", base_platform());
    }

    let mut tidc = 0;
    match clone(
        CloneFlags::CHILD_SETTID,
        Some(SIGCHLD),
        None,
        None,
        null_mut(),
        Some(&mut tidc),
    )
    .expect("init: failed to clone")
    {
        Some(tid) => {
            println!("init: forked child process with tid {}", tid);
            loop {
                let mut wstatus = WStatusRaw::EMPTY;
                match wait4(WaitFor::AnyChild, Some(&mut wstatus), WaitOptions::empty()) {
                    Ok(Some(tid)) => {
                        /*println!(
                            "init: child task #{} exited with code {:?}",
                            tid,
                            wstatus.read()
                        )；*/
                    },
                    Ok(None) => {
                        panic!(
                            "init: wait4 returned None but no error, this should not happen, since we didn't specify WNOHANG"
                        );
                    },
                    Err(EINTR) => {
                        // wait4 may be interrupted by SIGCHLD or another signal
                        // before the exited child is reaped. PID 1 must retry
                        // instead of turning a recoverable Linux ABI result into
                        // an init-exit panic.
                        continue;
                    },
                    Err(ECHILD) => {
                        sched_yield().expect("init: failed to yield");
                    },
                    Err(e) => {
                        panic!("init: cannot recycle child tasks: {}", e);
                    },
                }
            }
        },
        None => {
            // child
            log_execve_payload(USER_TEST_PATH, USER_TEST_ARGV, USER_TEST_ENVP);
            execve(USER_TEST_PATH, USER_TEST_ARGV, USER_TEST_ENVP)
                .expect("init: failed to execve user-test");
            unreachable!();
        },
    }
}
