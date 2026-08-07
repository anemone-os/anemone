//! Linux timerfd ABI conversion.

use anemone_abi::time::linux::{ITimerSpec, TimeSpec};

use crate::prelude::*;

const NSEC_PER_SEC: u64 = 1_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TimerFdSettimeFlags {
    pub(super) abstime: bool,
    pub(super) cancel_on_set: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TimerFdSpec {
    pub(super) value_ns: u64,
    pub(super) interval_ns: Option<u64>,
}

impl TryFrom<ITimerSpec> for TimerFdSpec {
    type Error = SysError;

    fn try_from(value: ITimerSpec) -> Result<Self, Self::Error> {
        let value_ns = timespec_to_ns(value.it_value)?;
        let interval_ns = timespec_to_ns(value.it_interval)?;
        Ok(Self {
            value_ns,
            interval_ns: (interval_ns != 0).then_some(interval_ns),
        })
    }
}

impl From<TimerFdSpec> for ITimerSpec {
    fn from(value: TimerFdSpec) -> Self {
        Self {
            it_interval: ns_to_timespec(value.interval_ns.unwrap_or(0)),
            it_value: ns_to_timespec(value.value_ns),
        }
    }
}

fn timespec_to_ns(ts: TimeSpec) -> Result<u64, SysError> {
    if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= NSEC_PER_SEC as i64 {
        return Err(SysError::InvalidArgument);
    }
    let sec_ns = (ts.tv_sec as u64)
        .checked_mul(NSEC_PER_SEC)
        .ok_or(SysError::InvalidArgument)?;
    sec_ns
        .checked_add(ts.tv_nsec as u64)
        .ok_or(SysError::InvalidArgument)
}

fn ns_to_timespec(ns: u64) -> TimeSpec {
    TimeSpec {
        tv_sec: (ns / NSEC_PER_SEC) as i64,
        tv_nsec: (ns % NSEC_PER_SEC) as i64,
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn rejects_invalid_or_overflowing_timespec_fields() {
        for invalid in [
            TimeSpec {
                tv_sec: -1,
                tv_nsec: 0,
            },
            TimeSpec {
                tv_sec: 0,
                tv_nsec: -1,
            },
            TimeSpec {
                tv_sec: 0,
                tv_nsec: NSEC_PER_SEC as i64,
            },
            TimeSpec {
                tv_sec: i64::MAX,
                tv_nsec: 0,
            },
        ] {
            assert_eq!(timespec_to_ns(invalid), Err(SysError::InvalidArgument));
        }
    }

    #[kunit]
    fn internal_spec_round_trips_linux_layout() {
        let abi = ITimerSpec {
            it_interval: TimeSpec {
                tv_sec: 2,
                tv_nsec: 3,
            },
            it_value: TimeSpec {
                tv_sec: 4,
                tv_nsec: 5,
            },
        };
        let spec = TimerFdSpec::try_from(abi).unwrap();
        assert_eq!(ITimerSpec::from(spec), abi);
    }
}
