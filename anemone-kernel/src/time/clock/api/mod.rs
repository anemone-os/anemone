pub mod clock_adjtime;
pub mod clock_getres;
pub mod clock_gettime;
pub mod clock_nanosleep;
pub mod clock_settime;

use anemone_abi::time::linux::{TimeSpec, TimeVal};

use crate::{prelude::*, time::timekeeper::RealtimeStep};

const NSEC_PER_SEC: u64 = 1_000_000_000;

fn timespec_to_ns(ts: TimeSpec) -> Result<u64, SysError> {
    if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= NSEC_PER_SEC as i64 {
        return Err(SysError::InvalidArgument);
    }
    (ts.tv_sec as u64)
        .checked_mul(NSEC_PER_SEC)
        .and_then(|seconds| seconds.checked_add(ts.tv_nsec as u64))
        .ok_or(SysError::InvalidArgument)
}

fn ns_to_timespec(ns: u64) -> TimeSpec {
    TimeSpec {
        tv_sec: (ns / NSEC_PER_SEC) as i64,
        tv_nsec: (ns % NSEC_PER_SEC) as i64,
    }
}

fn ns_to_timeval(ns: u64) -> TimeVal {
    TimeVal {
        tv_sec: (ns / NSEC_PER_SEC) as i64,
        tv_usec: ((ns % NSEC_PER_SEC) / 1_000) as i64,
    }
}

fn ns_to_duration(ns: u64) -> Duration {
    Duration::from_secs(ns / NSEC_PER_SEC) + Duration::from_nanos(ns % NSEC_PER_SEC)
}

fn publish_realtime_step(step: Option<RealtimeStep>) {
    let Some(_step) = step else {
        return;
    };
    // The timekeeper mutation returned only after releasing its lock. Timer
    // queues and object callbacks are therefore reached strictly lock-outside.
    crate::time::timer::recheck_realtime_requests();
}
