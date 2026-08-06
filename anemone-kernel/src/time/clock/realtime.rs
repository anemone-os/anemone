use crate::{prelude::*, time::clock::Clock};

pub struct RealtimeClock;

impl Clock for RealtimeClock {
    fn resolution_ns(&self) -> u64 {
        source_resolution_ns()
    }

    fn now_ns(&self) -> u64 {
        realtime_ns()
    }
}

pub static REALTIME: RealtimeClock = RealtimeClock;
