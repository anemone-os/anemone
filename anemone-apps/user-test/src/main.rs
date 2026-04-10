#![no_std]
#![no_main]
#![warn(unused)]

use core::ptr::null_mut;

use anemone_rs::{
    env::current_dir,
    os::linux::process::{clone, execve, getpid, CloneFlags},
    prelude::*,
};

static TEST_POINTS: &[&str] = &["wait", "waitpid", "uname"];

#[main]
pub fn main() -> Result<(), Errno> {
    let cwd = current_dir()?;
    println!("user-test: current working directory: {}", cwd.display());

    for p in TEST_POINTS {
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
            execve(&format!("/{}", p), &[&format!("/{}", p), "1"])
                .expect("failed to execve test point");
            unreachable!();
        } else {
            println!("user-test: test point '{}' started with pid {}", p, tid);
        }
    }

    // clone(CloneFlags::empty(), None, None, null_mut(), None)?;
    // for i in 0..20 {
    //     println!("Hello from user task #{}:{}!", getpid().unwrap(), i);
    // }
    Ok(())
}
