#![no_std]
#![no_main]
#![warn(unused)]

use anemone_rs::prelude::*;

#[main]
pub fn main() -> Result<(), Errno> {
    let cwd = anemone_rs::env::current_dir()?;
    println!("user-test: current working directory: {}", cwd.display());

    // tmp test
    anemone_rs::os::linux::process::execve("/uname", &["/uname"]).unwrap();
    Ok(())
}
