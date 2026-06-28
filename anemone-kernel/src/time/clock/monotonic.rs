use crate::{prelude::*, time::clock::Clock};

#[derive(Debug)]
pub struct MonotonicClock;

impl Clock for MonotonicClock {
    fn now_ns(&self) -> u64 {
        // the same as monotonic_raw. we dont' have ntp or something like that.
        Instant::now().to_duration().as_nanos() as u64
    }
}

pub static MONOTONIC: MonotonicClock = MonotonicClock;
