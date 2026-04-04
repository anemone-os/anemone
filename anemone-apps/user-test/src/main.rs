#![no_std]
#![no_main]
#![warn(unused)]

use anemone_rs::{prelude::*, syscalls::sys_clone};

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    sys_clone()?;
    for i in 0..10 {
        println!("Hello from user task {}!", i);
    }
    Ok(())
}

