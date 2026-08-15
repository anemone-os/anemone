#![no_std]
#![no_main]

use anemone_rs::{
    abi::system::native::power::SHUTDOWN_MAGIC, os::anemone::power::shutdown, prelude::*,
};

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    println!("NEMOPHILA-R0:NEGATIVE:INITIAL-USERSPACE-REACHED");
    shutdown(SHUTDOWN_MAGIC)?;
    unreachable!("nemophila boot-negative sentinel: shutdown returned unexpectedly")
}
