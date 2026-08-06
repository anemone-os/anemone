use crate::{prelude::*, time::clock::Clock};

#[derive(Debug)]
pub struct MonotonicCoarseClock;

impl Clock for MonotonicCoarseClock {
    fn resolution_ns(&self) -> u64 {
        coarse_resolution_ns()
    }

    fn now_ns(&self) -> u64 {
        coarse_monotonic_ns()
    }
}

pub static MONOTONIC_COARSE: MonotonicCoarseClock = MonotonicCoarseClock;
