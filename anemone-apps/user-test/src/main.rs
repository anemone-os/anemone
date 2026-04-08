#![no_std]
#![no_main]
#![warn(unused)]

use core::ptr::null_mut;

use anemone_rs::{
    env::current_dir,
    os::linux::process::{CloneFlags, clone, getpid, wait4},
    prelude::*,
};

#[main]
pub fn main() -> Result<(), Errno> {
    let cwd = current_dir()?;
    println!("user-test: current working directory: {}", cwd.display());
    let mut parent_tid = 0;
    let mut __child_tid = 0;
    clone(
        CloneFlags::CLONE_PARENT_SETTID | CloneFlags::CLONE_CHILD_SETTID,
        None,
        &mut parent_tid,
        null_mut(),
        &mut __child_tid,
    )?;
    for i in 0..20 {
        println!("Hello from user task #{}:{}!", getpid().unwrap(), i);
    }
    if parent_tid != 0 {
        wait4(parent_tid as i64).unwrap();
    }
    Ok(())
}
