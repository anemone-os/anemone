//! Anemone kernel.

#![no_std]
#![no_main]
#![allow(unused)]
// **IMPORTANT**
// **UNSTABLE FEATURES SHOULD BE AVOIDED WHENEVER POSSIBLE, SINCE THEY MAY CAUSE
// COMPATIBILITY ISSUES IN THE FUTURE.**

extern crate alloc;

pub mod kconfig_defs;
pub mod platform_defs;

pub mod prelude;

pub mod arch;
pub mod debug;
pub mod device;
pub mod driver;
pub mod exception;
pub mod mm;
pub mod panic;
pub mod power;
pub mod sched;
pub mod sync;
pub mod syscall;
pub mod syserror;
pub mod time;
pub mod utils;

use crate::prelude::*;

pub fn kernel_main(is_bsp: bool) -> ! {
    // TODO: init subsystems, spawn init process, etc.

    #[cfg(feature = "kunit")]
    if is_bsp {
        crate::debug::kunit::kunit_runner();
    }

    // TODO: start the scheduler, which should never return.

    loop {}
}

#[kunit]
fn kunit_example() {
    assert_eq!(1 + 1, 2);
}
