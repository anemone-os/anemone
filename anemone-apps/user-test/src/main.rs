#![no_std]
#![no_main]
#![warn(unused)]

use anemone_rs::{
    fs::getcwd,
    prelude::*,
    process::{clone, getpid},
};

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    let cwd = getcwd()?;
    println!("user-test: current working directory: {}", cwd);
    let mut __parent_ptid = 0;
    let mut __child_ptid = 0;
    clone(&mut __parent_ptid, &mut __child_ptid)?;
    for i in 0..10 {
        println!("Hello from user task #{}:{}!", getpid().unwrap(), i);
    }
    Ok(())
}
