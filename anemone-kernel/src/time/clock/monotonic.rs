use crate::{prelude::*, time::clock::Clock};

#[derive(Debug)]
pub struct MonotonicClock;

impl Clock for MonotonicClock {
    fn resolution_ns(&self) -> u64 {
        source_resolution_ns()
    }

    fn now_ns(&self) -> u64 {
        monotonic_ns()
    }
}

pub static MONOTONIC: MonotonicClock = MonotonicClock;
