use crate::{prelude::*, time::clock::Clock};

#[derive(Debug)]
pub struct RealtimeCoarseClock;

impl Clock for RealtimeCoarseClock {
    fn resolution_ns(&self) -> u64 {
        coarse_resolution_ns()
    }

    fn now_ns(&self) -> u64 {
        coarse_realtime_ns()
    }
}

pub static REALTIME_COARSE: RealtimeCoarseClock = RealtimeCoarseClock;
