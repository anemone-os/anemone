use crate::{prelude::*, time::clock::Clock};

#[derive(Debug)]
pub struct MonotonicCoarseClock;

impl Clock for MonotonicCoarseClock {
    fn now_ns(&self) -> u64 {
        Instant::now().to_duration().as_nanos() as u64
    }
}

pub static MONOTONIC_COARSE: MonotonicCoarseClock = MonotonicCoarseClock;
