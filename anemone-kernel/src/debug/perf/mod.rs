//! Low-overhead, observation-only kernel performance metrics.

#[cfg(feature = "perf_observe")]
pub mod api;
#[cfg(feature = "perf_observe")]
mod registry;
mod timer;

#[cfg(feature = "perf_observe")]
pub(crate) use registry::{MetricRegistration, MetricUnit, validate_registry};
#[cfg(feature = "perf_observe")]
use registry::{
    catalog_layout, recording_enabled, replace_recording_enabled, snapshot_values,
    try_for_each_metric,
};
pub(crate) use timer::TimerGuard;

#[cfg(feature = "perf_observe")]
use crate::prelude::*;
#[cfg(feature = "perf_observe")]
use anemone_abi::system::native::perf::{PERF_HISTOGRAM_SUM_INDEX, PERF_HISTOGRAM_VALUE_COUNT};

#[cfg(feature = "perf_observe")]
fn __counter_add(storage: &'static PerCpu<AtomicU64>, delta: u64) {
    storage.with(|value| {
        value.fetch_add(delta, Ordering::Relaxed);
    });
}

#[cfg(feature = "perf_observe")]
fn __histogram_record(
    storage: &'static PerCpu<[AtomicU64; PERF_HISTOGRAM_VALUE_COUNT]>,
    sample: u64,
) {
    let bucket = if sample == 0 {
        0
    } else {
        u64::BITS as usize - sample.leading_zeros() as usize
    };
    storage.with(|values| {
        values[bucket].fetch_add(1, Ordering::Relaxed);
        values[PERF_HISTOGRAM_SUM_INDEX].fetch_add(sample, Ordering::Relaxed);
    });
}

#[cfg(feature = "perf_observe")]
#[doc(hidden)]
pub(crate) fn __counter_add_if_enabled(
    storage: &'static PerCpu<AtomicU64>,
    delta: impl FnOnce() -> u64,
) {
    if recording_enabled() {
        __counter_add(storage, delta());
    }
}

#[cfg(feature = "perf_observe")]
#[doc(hidden)]
pub(crate) fn __histogram_record_if_enabled(
    storage: &'static PerCpu<[AtomicU64; PERF_HISTOGRAM_VALUE_COUNT]>,
    sample: impl FnOnce() -> u64,
) {
    if recording_enabled() {
        __histogram_record(storage, sample());
    }
}

#[cfg(feature = "perf_observe")]
#[doc(hidden)]
pub(crate) fn __timer_if_enabled(
    storage: &'static PerCpu<[AtomicU64; PERF_HISTOGRAM_VALUE_COUNT]>,
) -> TimerGuard {
    if recording_enabled() {
        TimerGuard::__new(storage, crate::time::perf_clock_ticks())
    } else {
        TimerGuard::__disabled()
    }
}

#[cfg(not(feature = "perf_observe"))]
#[doc(hidden)]
pub(crate) const fn __disabled_timer() -> TimerGuard {
    TimerGuard::__disabled()
}

#[cfg(feature = "perf_observe")]
#[macro_export]
macro_rules! declare_perf_metrics {
    ($(counter $counter:ident { name: $counter_name:literal, unit: $counter_unit:ident, })*
     $(histogram $histogram:ident { name: $histogram_name:literal, unit: $histogram_unit:ident, })*) => {
        $(
            #[percpu]
            static $counter: core::sync::atomic::AtomicU64 =
                core::sync::atomic::AtomicU64::new(0);
            ::paste::paste! {
                #[used]
                #[unsafe(link_section = ".perf_metrics")]
                static [<__PERF_REG_ $counter>]: $crate::debug::perf::MetricRegistration =
                    $crate::debug::perf::MetricRegistration::counter(
                        $counter_name,
                        $crate::debug::perf::MetricUnit::$counter_unit,
                        &$counter,
                    );
            }
        )*
        $(
            #[percpu]
            static $histogram: [core::sync::atomic::AtomicU64;
                anemone_abi::system::native::perf::PERF_HISTOGRAM_VALUE_COUNT] =
                [const { core::sync::atomic::AtomicU64::new(0) };
                    anemone_abi::system::native::perf::PERF_HISTOGRAM_VALUE_COUNT];
            ::paste::paste! {
                #[used]
                #[unsafe(link_section = ".perf_metrics")]
                static [<__PERF_REG_ $histogram>]: $crate::debug::perf::MetricRegistration =
                    $crate::debug::perf::MetricRegistration::histogram(
                        $histogram_name,
                        $crate::debug::perf::MetricUnit::$histogram_unit,
                        &$histogram,
                    );
            }
        )*
    };
}

#[cfg(not(feature = "perf_observe"))]
#[macro_export]
macro_rules! declare_perf_metrics {
    ($($tokens:tt)*) => {};
}

#[cfg(feature = "perf_observe")]
#[macro_export]
macro_rules! perf_counter_inc {
    ($metric:ident) => {{
        $crate::debug::perf::__counter_add_if_enabled(&$metric, || 1);
    }};
}

#[cfg(not(feature = "perf_observe"))]
#[macro_export]
macro_rules! perf_counter_inc {
    ($metric:ident) => {{}};
}

#[cfg(feature = "perf_observe")]
#[macro_export]
macro_rules! perf_counter_add {
    ($metric:ident, $value:expr) => {{
        $crate::debug::perf::__counter_add_if_enabled(&$metric, || $value);
    }};
}

#[cfg(not(feature = "perf_observe"))]
#[macro_export]
macro_rules! perf_counter_add {
    ($metric:ident, $value:expr) => {{}};
}

#[cfg(feature = "perf_observe")]
#[macro_export]
macro_rules! perf_histogram_record {
    ($metric:ident, $value:expr) => {{
        $crate::debug::perf::__histogram_record_if_enabled(&$metric, || $value);
    }};
}

#[cfg(not(feature = "perf_observe"))]
#[macro_export]
macro_rules! perf_histogram_record {
    ($metric:ident, $value:expr) => {{}};
}

#[cfg(feature = "perf_observe")]
#[macro_export]
macro_rules! perf_timer {
    ($metric:ident) => {{ $crate::debug::perf::__timer_if_enabled(&$metric) }};
}

#[cfg(not(feature = "perf_observe"))]
#[macro_export]
macro_rules! perf_timer {
    ($metric:ident) => {{ $crate::debug::perf::__disabled_timer() }};
}

#[cfg(all(feature = "kunit", feature = "perf_observe"))]
mod kunits {
    use super::*;

    #[percpu]
    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);
    #[percpu]
    static TEST_HISTOGRAM: [AtomicU64; PERF_HISTOGRAM_VALUE_COUNT] =
        [const { AtomicU64::new(0) }; PERF_HISTOGRAM_VALUE_COUNT];

    static EVALUATED: AtomicU64 = AtomicU64::new(0);

    fn evaluated(value: u64) -> u64 {
        EVALUATED.fetch_add(1, Ordering::Relaxed);
        value
    }

    #[kunit]
    fn runtime_gate_precedes_argument_evaluation_and_updates_wrap() {
        let old = replace_recording_enabled(false);
        EVALUATED.store(0, Ordering::Relaxed);
        perf_counter_add!(TEST_COUNTER, evaluated(1));
        perf_histogram_record!(TEST_HISTOGRAM, evaluated(1));
        assert_eq!(EVALUATED.load(Ordering::Relaxed), 0);

        TEST_COUNTER.with(|counter| counter.store(u64::MAX, Ordering::Relaxed));
        replace_recording_enabled(true);
        perf_counter_add!(TEST_COUNTER, evaluated(1));
        assert_eq!(EVALUATED.load(Ordering::Relaxed), 1);
        TEST_COUNTER.with(|counter| assert_eq!(counter.load(Ordering::Relaxed), 0));
        replace_recording_enabled(old);
    }

    #[kunit]
    fn histogram_covers_full_u64_domain() {
        let clear = || {
            TEST_HISTOGRAM.with(|buckets| {
                for bucket in buckets {
                    bucket.store(0, Ordering::Relaxed);
                }
            });
        };

        clear();
        __histogram_record(&TEST_HISTOGRAM, 0);
        TEST_HISTOGRAM.with(|values| {
            assert_eq!(values[0].load(Ordering::Relaxed), 1);
            assert_eq!(values[PERF_HISTOGRAM_SUM_INDEX].load(Ordering::Relaxed), 0);
        });

        for exponent in 0..u64::BITS as usize {
            clear();
            let power = 1u64 << exponent;
            __histogram_record(&TEST_HISTOGRAM, power);
            TEST_HISTOGRAM.with(|values| {
                assert_eq!(values[exponent + 1].load(Ordering::Relaxed), 1);
                assert_eq!(
                    values[PERF_HISTOGRAM_SUM_INDEX].load(Ordering::Relaxed),
                    power
                );
            });
            if exponent != 0 {
                __histogram_record(&TEST_HISTOGRAM, power - 1);
                TEST_HISTOGRAM.with(|values| {
                    assert_eq!(values[exponent].load(Ordering::Relaxed), 1);
                    assert_eq!(
                        values[PERF_HISTOGRAM_SUM_INDEX].load(Ordering::Relaxed),
                        power.wrapping_add(power - 1)
                    );
                });
            }
        }

        clear();
        __histogram_record(&TEST_HISTOGRAM, u64::MAX);
        __histogram_record(&TEST_HISTOGRAM, 1);
        TEST_HISTOGRAM.with(|values| {
            assert_eq!(values[64].load(Ordering::Relaxed), 1);
            assert_eq!(values[1].load(Ordering::Relaxed), 1);
            assert_eq!(values[PERF_HISTOGRAM_SUM_INDEX].load(Ordering::Relaxed), 0);
        });
    }
}

#[cfg(all(feature = "kunit", not(feature = "perf_observe")))]
mod compile_disabled_kunits {
    use crate::prelude::*;

    static EVALUATED: AtomicU64 = AtomicU64::new(0);

    fn evaluated() -> u64 {
        EVALUATED.fetch_add(1, Ordering::Relaxed);
        1
    }

    #[kunit]
    fn compile_disabled_macros_do_not_resolve_metrics_or_evaluate_arguments() {
        EVALUATED.store(0, Ordering::Relaxed);
        perf_counter_inc!(NO_COUNTER_STORAGE);
        perf_counter_add!(NO_COUNTER_STORAGE, evaluated());
        perf_histogram_record!(NO_HISTOGRAM_STORAGE, evaluated());
        perf_timer!(NO_HISTOGRAM_STORAGE).finish();
        assert_eq!(EVALUATED.load(Ordering::Relaxed), 0);
    }
}
