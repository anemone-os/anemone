//! Time helpers for runner-local deadlines.
//!
//! The runner uses wall-clock microseconds only for user-test diagnostics and
//! soft containment deadlines. These helpers must not be treated as kernel time
//! policy or as proof that the kernel-side wait/cleanup hang is fixed.

use anemone_rs::{os::linux::time::gettimeofday, prelude::*};

pub(super) const MICROS_PER_SECOND: i64 = 1_000_000;

pub(super) fn now_us() -> Result<i64, Errno> {
    let tv = gettimeofday()?;
    Ok(tv
        .tv_sec
        .saturating_mul(MICROS_PER_SECOND)
        .saturating_add(tv.tv_usec))
}

pub(super) fn elapsed_us_since(start_us: i64) -> Result<i64, Errno> {
    Ok(now_us()?.saturating_sub(start_us))
}
