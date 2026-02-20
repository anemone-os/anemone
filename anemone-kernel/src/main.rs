//! Anemone kernel.

#![no_std]
#![no_main]
#![allow(unused)]

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
pub mod syserror;
pub mod time;
pub mod utils;

use crate::prelude::*;

pub fn kernel_main() -> ! {
    loop {}
}
