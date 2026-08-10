#![no_std]
#![no_main]

mod clock_read;
mod clock_step;
mod posix_timer;
mod soft_timer;

use anemone_rs::prelude::*;

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    let initial = clock_read::verify_boot_walltime();
    clock_read::verify_native_clocks();
    clock_step::verify_clock_steps(initial);
    soft_timer::verify_soft_timer_consumers();
    posix_timer::verify_posix_timers();
    println!(
        "{}",
        yansi::Paint::green("clock_tests: clock/time/timer checks passed")
    );
    Ok(())
}
