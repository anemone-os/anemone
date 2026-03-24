//! Anemone kernel.

#![no_std]
#![no_main]
#![allow(unused)]
#![warn(unused_imports)]
// **IMPORTANT**
// **UNSTABLE FEATURES SHOULD BE AVOIDED WHENEVER POSSIBLE, SINCE THEY MAY CAUSE
// COMPATIBILITY ISSUES IN THE FUTURE.**
// **EVERY TIME A NEW UNSTABLE FEATURE IS ADDED, IT SHOULD BE DOCUMENTED.**

// This feature must be enabled for zero-cost downcasting of trait objects to get the same
// efficiency as C's void* and manual casts, which is crucial for the performance of the kernel.
#![feature(downcast_unchecked)]

extern crate alloc;

pub mod kconfig_defs;
pub mod platform_defs;

pub mod prelude;

pub mod arch;
pub mod debug;
pub mod device;
pub mod driver;
pub mod exception;
pub mod initcall;
pub mod mm;
pub mod panic;
pub mod power;
pub mod sched;
pub mod sync;
pub mod syscall;
pub mod syserror;
pub mod task;
pub mod time;
pub mod utils;

use crate::{prelude::*, sync::mono::MonoOnce};

