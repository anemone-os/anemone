use anemone_net_api::{Duration, Instant};

pub(crate) struct ManualClock {
    pub(crate) now: Instant,
}

impl ManualClock {
    pub(crate) fn new() -> Self {
        Self { now: Instant::ZERO }
    }

    pub(crate) fn advance(&mut self, duration: Duration) {
        self.now = Instant::from_micros(
            self.now.total_micros() + i64::try_from(duration.total_micros()).unwrap(),
        );
    }
}
