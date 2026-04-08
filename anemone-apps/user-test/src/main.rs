#![no_std]
#![no_main]
#![warn(unused)]

use core::ptr::null_mut;

use anemone_rs::{
    env::current_dir,
    os::linux::process::{CloneFlags, clone, getpid},
    prelude::*,
};

#[main]
pub fn main() -> Result<(), Errno> {
    let cwd = current_dir()?;
    println!("user-test: current working directory: {}", cwd.display());
    let mut __parent_ptid = 0;
    let mut __child_ptid = 0;
    clone(
        CloneFlags::CLONE_PARENT_SETTID | CloneFlags::CLONE_CHILD_SETTID,
        None,
        &mut __parent_ptid,
        null_mut(),
        &mut __child_ptid,
    )?;
    for i in 0..10 {
        println!("Hello from user task #{}:{}!", getpid().unwrap(), i);
    }
    Ok(())
}
