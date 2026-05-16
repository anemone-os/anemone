//! POSIX & Linux clock.

use crate::time::clock::{
    monotonic::MonotonicClock, monotonic_coarse::MonotonicCoarseClock,
    process_cputime::ProcessCpuTimeClock, realtime::RealtimeClock,
    realtime_coarse::RealtimeCoarseClock, thread_cputime::ThreadCpuTimeClock,
};

pub trait Clock: Sync {
    /// The resolution of the clock in nanoseconds.
    ///
    /// 1 nanosecond resolution as a default value.
    ///
    /// TODO: when we have an rtc device subsystem, remove this default
    /// placeholder.
    fn resolution_ns(&self) -> u64 {
        1
    }

    /// We use nanoseconds as the unit of time, which should be sufficient for
    /// all kinds of clocks.
    fn now_ns(&self) -> u64;

    // TODO: create timer, etc.
}

mod monotonic;
mod monotonic_coarse;
mod monotonic_raw;
mod process_cputime;
mod realtime;
mod realtime_coarse;
mod thread_cputime;

mod api;
#[allow(unused_imports)]
pub use api::*;

static STATIC_CLOCKS: &[&dyn Clock] = &[
    // note the index.
    &RealtimeClock,
    &MonotonicClock,
    &ProcessCpuTimeClock,
    &ThreadCpuTimeClock,
    &MonotonicClock,
    &RealtimeCoarseClock,
    &MonotonicCoarseClock,
];

// TODO: dynamic registration of clocks. e.g. for those dynamically-created rtc
// clocks.

/// Get a clock by its ID.
pub fn get_clock(clock_id: usize) -> Option<&'static dyn Clock> {
    STATIC_CLOCKS.get(clock_id).copied()
}
