//! TODO: refactor redundant code.

#[cfg(target_arch = "riscv64")]
pub mod getrlimit;
pub mod getrusage;
pub mod prlimit64;

use crate::prelude::*;
