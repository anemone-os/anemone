use anemone_abi::system::native::perf::*;

use crate::prelude::*;

static RECORDING_ENABLED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub(crate) enum MetricKind {
    Counter = PERF_METRIC_COUNTER,
    Histogram = PERF_METRIC_HISTOGRAM,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub(crate) enum MetricUnit {
    Events = PERF_UNIT_EVENTS,
    MonotonicTicks = PERF_UNIT_MONOTONIC_TICKS,
}

#[derive(Debug)]
enum MetricStorage {
    Counter(&'static PerCpu<AtomicU64>),
    Histogram(&'static PerCpu<[AtomicU64; PERF_HISTOGRAM_BUCKET_COUNT]>),
}

#[derive(Debug)]
#[repr(C)]
pub(crate) struct MetricRegistration {
    name: &'static str,
    kind: MetricKind,
    unit: MetricUnit,
    storage: MetricStorage,
}

impl MetricRegistration {
    pub(crate) const fn counter(
        name: &'static str,
        unit: MetricUnit,
        storage: &'static PerCpu<AtomicU64>,
    ) -> Self {
        Self {
            name,
            kind: MetricKind::Counter,
            unit,
            storage: MetricStorage::Counter(storage),
        }
    }

    pub(crate) const fn histogram(
        name: &'static str,
        unit: MetricUnit,
        storage: &'static PerCpu<[AtomicU64; PERF_HISTOGRAM_BUCKET_COUNT]>,
    ) -> Self {
        Self {
            name,
            kind: MetricKind::Histogram,
            unit,
            storage: MetricStorage::Histogram(storage),
        }
    }

    pub(crate) fn name(&self) -> &'static str {
        self.name
    }

    pub(crate) fn kind(&self) -> MetricKind {
        self.kind
    }

    pub(crate) fn unit(&self) -> MetricUnit {
        self.unit
    }

    pub(crate) fn value_count(&self) -> usize {
        match self.storage {
            MetricStorage::Counter(_) => 1,
            MetricStorage::Histogram(_) => PERF_HISTOGRAM_BUCKET_COUNT,
        }
    }

    fn aggregate_into(&self, values: &mut [u64]) {
        assert_eq!(values.len(), self.value_count());
        for logical_id in 0..ncpus() {
            let cpu = CpuId::new(logical_id);
            match self.storage {
                MetricStorage::Counter(storage) => {
                    let read = |value: &AtomicU64| value.load(Ordering::Relaxed);
                    let value = if cpu == cur_cpu_id() {
                        storage.with(read)
                    } else {
                        // Atomic cells are the storage owner's explicit remote-read
                        // capability; no non-atomic mutable access aliases this read.
                        unsafe { storage.with_remote(cpu, read) }
                    };
                    values[0] = values[0].wrapping_add(value);
                },
                MetricStorage::Histogram(storage) => {
                    let add = |buckets: &[AtomicU64; PERF_HISTOGRAM_BUCKET_COUNT]| {
                        for (total, bucket) in values.iter_mut().zip(buckets) {
                            *total = total.wrapping_add(bucket.load(Ordering::Relaxed));
                        }
                    };
                    if cpu == cur_cpu_id() {
                        storage.with(add);
                    } else {
                        unsafe { storage.with_remote(cpu, add) };
                    }
                },
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CatalogLayout {
    pub(crate) metric_count: usize,
    pub(crate) value_count: usize,
    pub(crate) name_bytes: usize,
    pub(crate) byte_len: usize,
}

fn registrations() -> &'static [MetricRegistration] {
    use crate::arch::link_symbols::{__eperf_metrics, __sperf_metrics};

    unsafe {
        let start = __sperf_metrics as *const () as usize;
        let end = __eperf_metrics as *const () as usize;
        assert!(start.is_multiple_of(align_of::<MetricRegistration>()));
        assert!((end - start).is_multiple_of(size_of::<MetricRegistration>()));
        core::slice::from_raw_parts(
            start as *const MetricRegistration,
            (end - start) / size_of::<MetricRegistration>(),
        )
    }
}

pub(super) fn try_for_each_metric<E>(
    mut f: impl FnMut(usize, usize, &MetricRegistration) -> Result<(), E>,
) -> Result<(), E> {
    let mut value_offset = 0;
    for (id, metric) in registrations().iter().enumerate() {
        f(id, value_offset, metric)?;
        value_offset += metric.value_count();
    }
    Ok(())
}

pub(super) fn catalog_layout() -> CatalogLayout {
    let mut value_count = 0usize;
    let mut name_bytes = 0usize;
    for metric in registrations() {
        value_count = value_count
            .checked_add(metric.value_count())
            .expect("performance metric value count overflow");
        name_bytes = name_bytes
            .checked_add(metric.name.len())
            .expect("performance metric name bytes overflow");
    }
    let descriptors = registrations()
        .len()
        .checked_mul(PERF_METRIC_DESCRIPTOR_SIZE)
        .expect("performance metric descriptor bytes overflow");
    let byte_len = PERF_CATALOG_HEADER_SIZE
        .checked_add(descriptors)
        .and_then(|bytes| bytes.checked_add(name_bytes))
        .expect("performance metric catalog bytes overflow");
    CatalogLayout {
        metric_count: registrations().len(),
        value_count,
        name_bytes,
        byte_len,
    }
}

pub(crate) fn validate_registry() {
    let layout = catalog_layout();
    assert!(
        layout.metric_count > 0,
        "perf_observe requires a production metric"
    );
    assert!(u32::try_from(layout.metric_count).is_ok());
    assert!(u32::try_from(layout.value_count).is_ok());
    assert!(u32::try_from(layout.name_bytes).is_ok());
    for (index, metric) in registrations().iter().enumerate() {
        assert!(!metric.name.is_empty(), "performance metric name is empty");
        assert!(u16::try_from(metric.name.len()).is_ok());
        assert!(u16::try_from(metric.value_count()).is_ok());
        assert_eq!(
            metric.kind == MetricKind::Counter,
            metric.value_count() == 1
        );
        for other in &registrations()[..index] {
            assert_ne!(metric.name, other.name, "duplicate performance metric name");
        }
    }
}

pub(super) fn recording_enabled() -> bool {
    RECORDING_ENABLED.load(Ordering::Relaxed)
}

pub(super) fn replace_recording_enabled(enabled: bool) -> bool {
    RECORDING_ENABLED.swap(enabled, Ordering::Relaxed)
}

pub(super) fn snapshot_values<E>(mut emit: impl FnMut(u64) -> Result<(), E>) -> Result<(), E> {
    const MAX_VALUES_PER_METRIC: usize = PERF_HISTOGRAM_BUCKET_COUNT;
    let mut aggregate = [0u64; MAX_VALUES_PER_METRIC];
    for metric in registrations() {
        let count = metric.value_count();
        aggregate[..count].fill(0);
        metric.aggregate_into(&mut aggregate[..count]);
        for value in &aggregate[..count] {
            emit(*value)?;
        }
    }
    Ok(())
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn production_registry_is_unique_and_layout_is_complete() {
        validate_registry();
        let layout = catalog_layout();
        assert_eq!(layout.metric_count, registrations().len());
        assert_eq!(
            layout.byte_len,
            PERF_CATALOG_HEADER_SIZE
                + PERF_METRIC_DESCRIPTOR_SIZE * layout.metric_count
                + layout.name_bytes
        );
        let mut expected_offset = 0;
        try_for_each_metric::<()>(|_id, offset, metric| {
            assert_eq!(offset, expected_offset);
            expected_offset += metric.value_count();
            Ok(())
        })
        .unwrap();
        assert_eq!(expected_offset, layout.value_count);
    }
}
