use crate::{prelude::*, time::clock::Clock};

#[derive(Debug)]
pub struct MonotonicRawClock;

impl Clock for MonotonicRawClock {
    fn now_ns(&self) -> u64 {
        Instant::now().to_duration().as_nanos() as u64
    }
}

pub static MONOTONIC_RAW: MonotonicRawClock = MonotonicRawClock;
