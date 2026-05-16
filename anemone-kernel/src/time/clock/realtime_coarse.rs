use crate::{prelude::*, time::clock::Clock};

#[derive(Debug)]
pub struct RealtimeCoarseClock;

impl Clock for RealtimeCoarseClock {
    fn now_ns(&self) -> u64 {
        Instant::now().to_duration().as_nanos() as u64
    }
}

pub static REALTIME_COARSE: RealtimeCoarseClock = RealtimeCoarseClock;
