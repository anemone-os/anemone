#![no_std]
#![no_main]

use anemone_rs::{
    abi::system::native::power::SHUTDOWN_MAGIC, os::anemone::power::shutdown, prelude::*,
};

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    shutdown(SHUTDOWN_MAGIC)?;
    unreachable!("shutdown syscall returned unexpectedly");
}
