#[cfg(feature = "perf_observe")]
use crate::prelude::*;
#[cfg(feature = "perf_observe")]
use anemone_abi::system::native::perf::{
    PERF_ELAPSED_SAMPLE_COUNT_INDEX, PERF_ELAPSED_VALUE_COUNT, PERF_HISTOGRAM_SUM_INDEX,
    PERF_HISTOGRAM_VALUE_COUNT,
};

#[cfg(feature = "perf_observe")]
pub(crate) struct TimerGuard {
    armed: Option<(
        &'static PerCpu<[AtomicU64; PERF_HISTOGRAM_VALUE_COUNT]>,
        u64,
    )>,
}

#[cfg(not(feature = "perf_observe"))]
pub(crate) struct TimerGuard;

#[cfg(feature = "perf_observe")]
pub(crate) struct ElapsedGuard {
    armed: Option<(&'static PerCpu<[AtomicU64; PERF_ELAPSED_VALUE_COUNT]>, u64)>,
}

#[cfg(not(feature = "perf_observe"))]
pub(crate) struct ElapsedGuard;

#[cfg(feature = "perf_observe")]
pub(crate) struct SyscallProfileGuard {
    armed: Option<(
        &'static PerCpu<[AtomicU64; PERF_ELAPSED_VALUE_COUNT]>,
        &'static PerCpu<[AtomicU64; PERF_ELAPSED_VALUE_COUNT]>,
        u64,
        u64,
    )>,
}

#[cfg(not(feature = "perf_observe"))]
pub(crate) struct SyscallProfileGuard;

impl TimerGuard {
    #[cfg(feature = "perf_observe")]
    #[doc(hidden)]
    pub(super) fn __new(
        storage: &'static PerCpu<[AtomicU64; PERF_HISTOGRAM_VALUE_COUNT]>,
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

impl ElapsedGuard {
    #[cfg(feature = "perf_observe")]
    #[doc(hidden)]
    pub(super) fn __new(
        storage: &'static PerCpu<[AtomicU64; PERF_ELAPSED_VALUE_COUNT]>,
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
        // An interval admitted while recording was enabled completes exactly
        // once even if the global gate changes while it is in flight.
        super::__elapsed_record(storage, elapsed);
    }
}

#[cfg(feature = "perf_observe")]
impl Drop for ElapsedGuard {
    fn drop(&mut self) {
        self.record();
    }
}

impl SyscallProfileGuard {
    #[cfg(feature = "perf_observe")]
    #[doc(hidden)]
    pub(super) fn __new(
        elapsed_storage: &'static PerCpu<[AtomicU64; PERF_ELAPSED_VALUE_COUNT]>,
        kernel_cpu_storage: &'static PerCpu<[AtomicU64; PERF_ELAPSED_VALUE_COUNT]>,
        elapsed_begin: u64,
        kernel_cpu_begin: u64,
    ) -> Self {
        Self {
            armed: Some((
                elapsed_storage,
                kernel_cpu_storage,
                elapsed_begin,
                kernel_cpu_begin,
            )),
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
        let Some((elapsed_storage, kernel_cpu_storage, elapsed_begin, kernel_cpu_begin)) =
            self.armed.take()
        else {
            return;
        };
        let elapsed = crate::time::perf_clock_ticks().wrapping_sub(elapsed_begin);
        // A returning raw syscall wrapper is still executing as the same
        // current task. Reacquire that narrow accounting owner here instead of
        // retaining an Arc across a potentially blocking syscall.
        let kernel_cpu = get_current_task()
            .cpu_usage_snapshot()
            .kernel_mono()
            .wrapping_sub(kernel_cpu_begin);
        // Both values describe one completed raw-wrapper invocation. Recording
        // them from one guard keeps their sample counts structurally paired.
        super::__elapsed_record(elapsed_storage, elapsed);
        super::__elapsed_record(kernel_cpu_storage, kernel_cpu);
    }
}

#[cfg(feature = "perf_observe")]
impl Drop for SyscallProfileGuard {
    fn drop(&mut self) {
        self.record();
    }
}

#[cfg(all(feature = "kunit", feature = "perf_observe"))]
mod kunits {
    use super::*;

    #[percpu]
    static TIMER_HISTOGRAM: [AtomicU64; PERF_HISTOGRAM_VALUE_COUNT] =
        [const { AtomicU64::new(0) }; PERF_HISTOGRAM_VALUE_COUNT];
    #[percpu]
    static SYSCALL_ELAPSED: [AtomicU64; PERF_ELAPSED_VALUE_COUNT] =
        [const { AtomicU64::new(0) }; PERF_ELAPSED_VALUE_COUNT];
    #[percpu]
    static SYSCALL_KERNEL_CPU: [AtomicU64; PERF_ELAPSED_VALUE_COUNT] =
        [const { AtomicU64::new(0) }; PERF_ELAPSED_VALUE_COUNT];

    fn samples() -> u64 {
        TIMER_HISTOGRAM.with(|buckets| {
            buckets[..PERF_HISTOGRAM_SUM_INDEX]
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

    #[kunit]
    fn syscall_guard_completes_one_paired_sample() {
        for storage in [&SYSCALL_ELAPSED, &SYSCALL_KERNEL_CPU] {
            storage.with(|values| {
                for value in values {
                    value.store(0, Ordering::Relaxed);
                }
            });
        }
        let kernel_cpu_begin = get_current_task().cpu_usage_snapshot().kernel_mono();
        SyscallProfileGuard::__new(
            &SYSCALL_ELAPSED,
            &SYSCALL_KERNEL_CPU,
            crate::time::perf_clock_ticks(),
            kernel_cpu_begin,
        )
        .finish();
        for storage in [&SYSCALL_ELAPSED, &SYSCALL_KERNEL_CPU] {
            storage.with(|values| {
                assert_eq!(
                    values[PERF_ELAPSED_SAMPLE_COUNT_INDEX].load(Ordering::Relaxed),
                    1
                );
            });
        }
    }
}
