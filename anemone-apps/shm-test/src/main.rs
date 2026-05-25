#![no_std]
#![no_main]

use anemone_rs::prelude::*;

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    println!("===== shm test =====");

    Ok(())
}
