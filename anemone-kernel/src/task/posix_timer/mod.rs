//! Thread-group-owned POSIX timer objects and ID namespace.
//!
//! Linux UAPI conversion stays in `time::posix_timer::api`. This module owns
//! timer IDs, schedules, generations, periodic accounting, and teardown; the
//! soft-timer and signal subsystems receive only one-shot capabilities.

mod namespace;
mod timer;

use crate::{prelude::*, task::sig::SigNo};

pub use namespace::PosixTimers;
// Keep the existing owner-facing transaction type path even though callers
// currently consume it only through `prepare_posix_timer` return inference.
#[allow(unused_imports)]
pub(crate) use namespace::PreparedPosixTimer;

const NSEC_PER_SEC: u64 = 1_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PosixTimerClock {
    Realtime,
    Monotonic,
    Boottime,
}

#[derive(Debug)]
pub(crate) enum PosixTimerNotification {
    None,
    DefaultSignal,
    Signal {
        no: SigNo,
        sigval: u64,
    },
    /// Syscall-transaction snapshot used only to reserve the private slot.
    /// The published timer retains the registration's weak exact identity,
    /// not this `Task` reference.
    ThreadSignal {
        target: Arc<Task>,
        no: SigNo,
        sigval: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct PosixTimerSetting {
    pub(crate) value_ns: u64,
    pub(crate) interval_ns: u64,
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::time::timer::queued_timer_count;

    #[kunit]
    fn replace_disarm_delete_and_bulk_cleanup_return_queue_to_baseline() {
        let owner = get_current_task().get_thread_group();
        owner.delete_all_posix_timers();
        let cpu = cur_cpu_id();
        let baseline = queued_timer_count(cpu);

        let prepared = owner
            .prepare_posix_timer(PosixTimerClock::Monotonic, PosixTimerNotification::None)
            .unwrap();
        let id = prepared.id();
        prepared.publish().unwrap();
        for seconds in 1..=64 {
            owner
                .posix_timer_settime(
                    id,
                    PosixTimerSetting {
                        value_ns: (3600 + seconds) * NSEC_PER_SEC,
                        interval_ns: 0,
                    },
                    false,
                )
                .unwrap();
            assert_eq!(queued_timer_count(cpu), baseline + 1);
        }
        owner
            .posix_timer_settime(id, PosixTimerSetting::default(), false)
            .unwrap();
        assert_eq!(queued_timer_count(cpu), baseline);
        owner.delete_posix_timer(id).unwrap();

        for _ in 0..2 {
            let prepared = owner
                .prepare_posix_timer(PosixTimerClock::Monotonic, PosixTimerNotification::None)
                .unwrap();
            let id = prepared.id();
            prepared.publish().unwrap();
            owner
                .posix_timer_settime(
                    id,
                    PosixTimerSetting {
                        value_ns: 3600 * NSEC_PER_SEC,
                        interval_ns: 0,
                    },
                    false,
                )
                .unwrap();
        }
        assert_eq!(queued_timer_count(cpu), baseline + 2);
        owner.delete_all_posix_timers();
        assert_eq!(queued_timer_count(cpu), baseline);
        assert!(owner.posix_timer_gettime(0).is_err());
        assert!(owner.posix_timer_gettime(1).is_err());
    }
}
