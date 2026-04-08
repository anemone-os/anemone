#![no_std]
#![no_main]

use core::ptr::null_mut;

use anemone_rs::{
    env::current_dir,
    os::linux::process::{CloneFlags, WStatusRaw, clone, execve, getpid, wait4},
    prelude::*,
};

#[main]
pub fn main() -> Result<(), anemone_abi::errno::Errno> {
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
        execve("bin/user-test", &["bin/user-test", "1"]).expect("failed to execve user-test");
        unreachable!();
    } else {
        println!("init: 'bin/user-test' started with pid {}", tid);
        loop {
            let mut wstatus = WStatusRaw::EMPTY;
            match wait4(-1, Some(&mut wstatus)) {
                Ok(tid) => println!("init: task #{} exited with code {:?}", tid, wstatus.read()),
                Err(e) => {
                    if e == ECHILD {
                        continue;
                    } else {
                        panic!("init: cannot recycle child tasks: {}", e);
                    }
                },
            }
        }
    }
}
