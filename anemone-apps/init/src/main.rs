#![no_std]
#![no_main]

use core::ptr::null_mut;

use anemone_rs::{
    abi::process::linux::signal::SIGCHLD,
    env::*,
    os::linux::process::{
        CloneFlags, WStatusRaw, WaitFor, WaitOptions, clone, execve, sched_yield, wait4,
    },
    prelude::*,
    process::process_id,
};

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
    run("/bin/float-test", &["user-test"], &[])?;
    run("/bin/user-test", &["user-test"], &[])?;
    Ok(())
}
pub fn run(app: &str, argv: &[&str], envp: &[&str]) -> Result<(), Errno> {
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
                        println!(
                            "init: child task #{} exited with code {:?}",
                            tid,
                            wstatus.read()
                        )
                    },
                    Ok(None) => {
                        panic!(
                            "init: wait4 returned None but no error, this should not happen, since we didn't specify WNOHANG"
                        );
                    },
                    Err(e) => {
                        if e != ECHILD {
                            panic!("init: cannot recycle child tasks: {}", e);
                        } else {
                            sched_yield().expect("init: failed to yield");
                        }
                    },
                }
            }
        },
        None => {
            // child
            execve(app, argv, envp).expect("init: failed to execve user-test");
            unreachable!();
        },
    }
}
