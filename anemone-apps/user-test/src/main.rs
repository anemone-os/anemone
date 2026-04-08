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
    clone(CloneFlags::empty(), None, None, null_mut(), None)?;
    for i in 0..20 {
        println!("Hello from user task #{}:{}!", getpid().unwrap(), i);
    }
    Ok(())
}
