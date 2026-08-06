use crate::{prelude::*, time::clock::Clock};

#[derive(Debug)]
pub struct MonotonicRawClock;

impl Clock for MonotonicRawClock {
    fn resolution_ns(&self) -> u64 {
        source_resolution_ns()
    }

    fn now_ns(&self) -> u64 {
        // RAW has an independent route. It shares the value only because R0
        // deliberately excludes correction and frequency discipline.
        monotonic_ns()
    }
}

pub static MONOTONIC_RAW: MonotonicRawClock = MonotonicRawClock;
