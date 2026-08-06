//! POSIX & Linux clock.

use crate::time::clock::{
    boottime::BoottimeClock, monotonic::MonotonicClock, monotonic_coarse::MonotonicCoarseClock,
    monotonic_raw::MonotonicRawClock, process_cputime::ProcessCpuTimeClock,
    realtime::RealtimeClock, realtime_coarse::RealtimeCoarseClock,
    thread_cputime::ThreadCpuTimeClock,
};
use anemone_abi::time::linux::clock::{
    CLOCK_BOOTTIME, CLOCK_MONOTONIC, CLOCK_PROCESS_CPUTIME_ID, CLOCK_REALTIME,
    CLOCK_THREAD_CPUTIME_ID,
};

use crate::prelude::*;

pub trait Clock: Sync {
    /// The resolution of the clock in nanoseconds.
    fn resolution_ns(&self) -> u64;

    /// We use nanoseconds as the unit of time, which should be sufficient for
    /// all kinds of clocks.
    fn now_ns(&self) -> u64;
}

mod boottime;
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
    // The array index is the native Linux clock ID. Keep independent objects for
    // clocks that currently share a value so later semantics cannot silently
    // change ABI routing through object aliases.
    &RealtimeClock,
    &MonotonicClock,
    &ProcessCpuTimeClock,
    &ThreadCpuTimeClock,
    &MonotonicRawClock,
    &RealtimeCoarseClock,
    &MonotonicCoarseClock,
    &BoottimeClock,
];

// Native clock IDs are a fixed ABI table. Dynamic device clocks would need a
// separate registration owner and are outside the current clock contract.

/// Get a clock by its ID.
pub fn get_clock(clock_id: usize) -> Option<&'static dyn Clock> {
    STATIC_CLOCKS.get(clock_id).copied()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SleepClock {
    Monotonic,
    Realtime,
}

pub(crate) fn get_sleep_clock(clock_id: i32) -> Result<SleepClock, SysError> {
    match clock_id {
        CLOCK_REALTIME => Ok(SleepClock::Realtime),
        CLOCK_MONOTONIC | CLOCK_BOOTTIME => Ok(SleepClock::Monotonic),
        CLOCK_PROCESS_CPUTIME_ID | CLOCK_THREAD_CPUTIME_ID => {
            // These IDs are valid clocks, but sleeping on CPU consumption needs
            // scheduler-driven timers that R0 deliberately excludes.
            // Log every rejected call: unlike ignored legacy flags, the syscall
            // fails, and the log must remain correlatable with userspace errno.
            knoticeln!(
                "clock_nanosleep: clock_id={} requires scheduler-driven CPU timers; errno=EOPNOTSUPP",
                clock_id
            );
            Err(SysError::NotSupported)
        },
        _ => Err(SysError::InvalidArgument),
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn all_native_clock_ids_have_explicit_read_routes() {
        for clock_id in 0..8 {
            let clock = get_clock(clock_id).expect("native clock route is missing");
            let _ = clock.now_ns();
            assert!(clock.resolution_ns() > 0);
        }
        assert!(get_clock(8).is_none());

        assert!(!core::ptr::eq(get_clock(1).unwrap(), get_clock(4).unwrap()));
        assert!(!core::ptr::eq(get_clock(1).unwrap(), get_clock(7).unwrap()));
    }

    #[kunit]
    fn ordinary_and_coarse_clocks_use_their_own_resolution_classes() {
        for clock_id in [0, 1, 2, 3, 4, 7] {
            assert_eq!(
                get_clock(clock_id).unwrap().resolution_ns(),
                source_resolution_ns()
            );
        }
        for clock_id in [5, 6] {
            assert_eq!(
                get_clock(clock_id).unwrap().resolution_ns(),
                coarse_resolution_ns()
            );
        }
    }
}
