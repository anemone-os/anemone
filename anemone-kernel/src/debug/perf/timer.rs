#[cfg(feature = "perf_observe")]
use crate::prelude::*;
#[cfg(feature = "perf_observe")]
use anemone_abi::system::native::perf::PERF_HISTOGRAM_BUCKET_COUNT;

#[cfg(feature = "perf_observe")]
pub(crate) struct TimerGuard {
    armed: Option<(
        &'static PerCpu<[AtomicU64; PERF_HISTOGRAM_BUCKET_COUNT]>,
        u64,
    )>,
}

#[cfg(not(feature = "perf_observe"))]
pub(crate) struct TimerGuard;

impl TimerGuard {
    #[cfg(feature = "perf_observe")]
    #[doc(hidden)]
    pub(super) fn __new(
        storage: &'static PerCpu<[AtomicU64; PERF_HISTOGRAM_BUCKET_COUNT]>,
        begin: u64,
    ) -> Self {
        Self {
            armed: Some((storage, begin)),
        }
    }

    #[doc(hidden)]
    pub(super) const fn __disabled() -> Self {
        #[cfg(feature = "perf_observe")]
        {
            Self { armed: None }
        }
        #[cfg(not(feature = "perf_observe"))]
        {
            Self
        }
    }

    pub(crate) fn finish(mut self) {
        #[cfg(feature = "perf_observe")]
        self.record();
    }

    #[cfg(feature = "perf_observe")]
    fn record(&mut self) {
        let Some((storage, begin)) = self.armed.take() else {
            return;
        };
        let elapsed = crate::time::perf_clock_ticks().wrapping_sub(begin);
        // Armed timers deliberately bypass the current gate: disabling only
        // rejects new samples and does not cancel an in-flight observation.
        super::__histogram_record(storage, elapsed);
    }
}

#[cfg(feature = "perf_observe")]
impl Drop for TimerGuard {
    fn drop(&mut self) {
        self.record();
    }
}

#[cfg(all(feature = "kunit", feature = "perf_observe"))]
mod kunits {
    use super::*;

    #[percpu]
    static TIMER_HISTOGRAM: [AtomicU64; PERF_HISTOGRAM_BUCKET_COUNT] =
        [const { AtomicU64::new(0) }; PERF_HISTOGRAM_BUCKET_COUNT];

    fn samples() -> u64 {
        TIMER_HISTOGRAM.with(|buckets| {
            buckets
                .iter()
                .map(|bucket| bucket.load(Ordering::Relaxed))
                .sum()
        })
    }

    #[kunit]
    fn disabled_finish_and_armed_finish_drop_semantics() {
        TIMER_HISTOGRAM.with(|buckets| {
            for bucket in buckets {
                bucket.store(0, Ordering::Relaxed);
            }
        });
        TimerGuard::__disabled().finish();
        assert_eq!(samples(), 0);

        TimerGuard::__new(&TIMER_HISTOGRAM, crate::time::perf_clock_ticks()).finish();
        assert_eq!(samples(), 1);

        let old = super::super::replace_recording_enabled(true);
        let guard = TimerGuard::__new(&TIMER_HISTOGRAM, crate::time::perf_clock_ticks());
        super::super::replace_recording_enabled(false);
        drop(guard);
        assert_eq!(samples(), 2);
        super::super::replace_recording_enabled(old);
    }
}
