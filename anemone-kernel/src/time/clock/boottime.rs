use crate::{prelude::*, time::clock::Clock};

#[derive(Debug)]
pub struct BoottimeClock;

impl Clock for BoottimeClock {
    fn resolution_ns(&self) -> u64 {
        source_resolution_ns()
    }

    fn now_ns(&self) -> u64 {
        // Anemone has no suspend accounting yet. BOOTTIME therefore shares the
        // monotonic value, but this distinct route preserves its Linux ABI
        // identity for the stage that introduces suspend semantics.
        monotonic_ns()
    }
}
