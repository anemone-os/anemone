use crate::{prelude::*, time::clock::Clock};

pub struct RealtimeClock;

impl Clock for RealtimeClock {
    fn now_ns(&self) -> u64 {
        // rtc hasn't been supported yet.
        Instant::now().to_duration().as_nanos() as u64
    }
}

pub static REALTIME: RealtimeClock = RealtimeClock;
