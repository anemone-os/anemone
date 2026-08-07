#![no_std]
#![no_main]

mod posix_timer;

use anemone_rs::prelude::*;

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    posix_timer::verify_posix_timers();
    println!(
        "{}",
        yansi::Paint::green("clock_tests: POSIX timer RFC checks passed")
    );
    Ok(())
}
