#![no_std]
#![no_main]

use core::ptr::null_mut;

use anemone_rs::{
    env::current_dir,
    os::linux::process::{
        clone, execve, getpid, sched_yield, wait4, CloneFlags, WStatusRaw, WaitOptions,
    },
    prelude::*,
};

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    let cwd = current_dir()?;
    let pid = getpid()?;
    println!("init: started:\n\tcwd:{}\n\tpid:{}", cwd.display(), pid);
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
        println!("init: get into cloned task {}", tidc);
        execve("bin/user-test", &["bin/user-test"]).expect("failed to execve mmap-test");
        unreachable!();
    } else {
        println!("init: 'bin/user-test' started with pid {}", tid);
        loop {
            let mut wstatus = WStatusRaw::EMPTY;
            match wait4(-1, Some(&mut wstatus), WaitOptions::empty()) {
                Ok(Some(tid)) => {
                    println!("init: task #{} exited with code {:?}", tid, wstatus.read())
                },
                Ok(None) => {
                    panic!("init: wait4 returned None but no error, this should not happen");
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
    }
}
