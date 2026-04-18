#![no_std]
#![no_main]
#![warn(unused)]

use core::ptr::null_mut;

use anemone_rs::{
    env::current_dir,
    os::linux::{
        fs::chdir,
        process::{CloneFlags, WStatusRaw, WaitOptions, clone, execve, wait4},
    },
    prelude::*,
};

//static TEST_POINTS: &[&str] = &["wait", "waitpid", "uname"];

static BASIC_TESTS: &[&str] = &[
    "brk",
    "chdir",
    "clone",
    "close",
    "dup",
    "dup2",
    "execve",
    "exit",
    "fork",
    "fstat",
    "getcwd",
    "getdents",
    "getpid",
    "getppid",
    "gettimeofday",
    "mkdir_",
    "mmap",
    "mount",
    "munmap",
    "open",
    "openat",
    "pipe",
    "read",
    "sleep",
    "times",
    "umount",
    "uname",
    "unlink",
    "wait",
    "waitpid",
    "write",
    "yield",
];

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    chdir("basic").unwrap();
    let cwd = current_dir()?;
    println!("user-test: current working directory: {}", cwd.display());

    for p in BASIC_TESTS {
        let mut tidc = 0;
        let tid = clone(
            CloneFlags::CLONE_CHILD_SETTID,
            None,
            None,
            null_mut(),
            Some(&mut tidc),
        )
        .unwrap();
        if tid == 0 {
            println!("user-test: test point '{}'", p);
            execve(&format!("{}", p), &[&format!("{}", p)]).expect("failed to execve test point");
            unreachable!();
        } else {
            println!("user-test: test point '{}' started with pid {}", p, tid);

            let mut wstatus = WStatusRaw::EMPTY;
            match wait4(tid as i64, Some(&mut wstatus), WaitOptions::empty()) {
                Ok(Some(_)) => {
                    println!(
                        "user-test: test point '{}' exited with code {:?}",
                        p,
                        wstatus.read()
                    )
                },
                Ok(None) => {
                    panic!("user-test: wait4 returned None but no error, this should not happen");
                },
                Err(e) => {
                    eprintln!("user-test: failed to wait for test point '{}': {}", p, e);
                },
            }
        }
    }

    eprintln!("user-test: all test points finished!");

    // clone(CloneFlags::empty(), None, None, null_mut(), None)?;
    // for i in 0..20 {
    //     println!("Hello from user task #{}:{}!", getpid().unwrap(), i);
    // }
    Ok(())
}
