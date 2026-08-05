use alloc::string::String;

use anemone_abi::system::native::perf::*;

use crate::{prelude::*, sys::anemone::debug};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfMetricKind {
    Counter,
    Histogram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfMetricUnit {
    Events,
    MonotonicTicks,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerfMetricDescriptor {
    pub id: u32,
    pub kind: PerfMetricKind,
    pub unit: PerfMetricUnit,
    pub value_offset: usize,
    pub value_count: usize,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerfCatalog {
    pub clock_frequency_hz: u64,
    pub histogram_bucket_count: usize,
    pub value_count: usize,
    pub metrics: Vec<PerfMetricDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerfSnapshot {
    pub begin_ticks: u64,
    pub end_ticks: u64,
    pub enabled: bool,
    pub values: Vec<u64>,
}

impl PerfSnapshot {
    pub fn wrapping_delta_from(&self, before: &Self) -> Result<Vec<u64>, Errno> {
        if self.values.len() != before.values.len() {
            return Err(EINVAL);
        }
        Ok(self
            .values
            .iter()
            .zip(&before.values)
            .map(|(after, before)| after.wrapping_sub(*before))
            .collect())
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, Errno> {
    let raw: [u8; 2] = bytes
        .get(offset..offset + 2)
        .ok_or(EINVAL)?
        .try_into()
        .unwrap();
    Ok(u16::from_ne_bytes(raw))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, Errno> {
    let raw: [u8; 4] = bytes
        .get(offset..offset + 4)
        .ok_or(EINVAL)?
        .try_into()
        .unwrap();
    Ok(u32::from_ne_bytes(raw))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, Errno> {
    let raw: [u8; 8] = bytes
        .get(offset..offset + 8)
        .ok_or(EINVAL)?
        .try_into()
        .unwrap();
    Ok(u64::from_ne_bytes(raw))
}

fn negotiated_image(op: u64) -> Result<Vec<u8>, Errno> {
    let required = debug::perf_observe(op, 0, 0, 0, 0)? as usize;
    if required == 0 {
        return Err(EINVAL);
    }
    let mut bytes = vec![0u8; required];
    let copied = debug::perf_observe(
        op,
        0,
        bytes.as_mut_ptr() as u64,
        bytes.len(),
        0,
    )? as usize;
    if copied != required {
        return Err(EINVAL);
    }
    Ok(bytes)
}

pub fn query() -> Result<PerfCatalog, Errno> {
    parse_catalog(&negotiated_image(PERF_OBSERVE_QUERY)?)
}

pub fn get_enabled() -> Result<bool, Errno> {
    match debug::perf_observe(PERF_OBSERVE_GET_ENABLED, 0, 0, 0, 0)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(EINVAL),
    }
}

/// Replace the global recording gate and return its complete old value.
/// Callers own restoration on every exit path; the kernel has no session owner.
pub fn set_enabled(enabled: bool) -> Result<bool, Errno> {
    match debug::perf_observe(PERF_OBSERVE_SET_ENABLED, u64::from(enabled), 0, 0, 0)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(EINVAL),
    }
}

pub fn snapshot() -> Result<PerfSnapshot, Errno> {
    parse_snapshot(&negotiated_image(PERF_OBSERVE_SNAPSHOT)?)
}

fn parse_catalog(bytes: &[u8]) -> Result<PerfCatalog, Errno> {
    if bytes.len() < PERF_CATALOG_HEADER_SIZE
        || read_u32(bytes, PERF_CATALOG_CLOCK_KIND_OFFSET)? != PERF_CLOCK_MONOTONIC_RAW
        || read_u32(bytes, PERF_CATALOG_RESERVED_OFFSET)? != 0
    {
        return Err(EINVAL);
    }
    let metric_count = read_u32(bytes, PERF_CATALOG_METRIC_COUNT_OFFSET)? as usize;
    let value_count = read_u32(bytes, PERF_CATALOG_VALUE_COUNT_OFFSET)? as usize;
    let histogram_bucket_count =
        read_u32(bytes, PERF_CATALOG_HISTOGRAM_BUCKET_COUNT_OFFSET)? as usize;
    if histogram_bucket_count != PERF_HISTOGRAM_BUCKET_COUNT {
        return Err(EINVAL);
    }
    let name_bytes = read_u32(bytes, PERF_CATALOG_NAME_BYTES_OFFSET)? as usize;
    let descriptor_bytes = metric_count
        .checked_mul(PERF_METRIC_DESCRIPTOR_SIZE)
        .ok_or(EINVAL)?;
    let names_base = PERF_CATALOG_HEADER_SIZE
        .checked_add(descriptor_bytes)
        .ok_or(EINVAL)?;
    if names_base.checked_add(name_bytes).ok_or(EINVAL)? != bytes.len() {
        return Err(EINVAL);
    }
    let names = &bytes[names_base..];
    let mut metrics = Vec::with_capacity(metric_count);
    for index in 0..metric_count {
        let base = PERF_CATALOG_HEADER_SIZE + index * PERF_METRIC_DESCRIPTOR_SIZE;
        if read_u32(bytes, base + PERF_METRIC_RESERVED_OFFSET)? != 0 {
            return Err(EINVAL);
        }
        let kind = match read_u16(bytes, base + PERF_METRIC_KIND_OFFSET)? {
            PERF_METRIC_COUNTER => PerfMetricKind::Counter,
            PERF_METRIC_HISTOGRAM => PerfMetricKind::Histogram,
            _ => return Err(EINVAL),
        };
        let unit = match read_u16(bytes, base + PERF_METRIC_UNIT_OFFSET)? {
            PERF_UNIT_EVENTS => PerfMetricUnit::Events,
            PERF_UNIT_MONOTONIC_TICKS => PerfMetricUnit::MonotonicTicks,
            _ => return Err(EINVAL),
        };
        let value_offset = read_u32(bytes, base + PERF_METRIC_VALUE_OFFSET_OFFSET)? as usize;
        let metric_value_count = read_u16(bytes, base + PERF_METRIC_VALUE_COUNT_OFFSET)? as usize;
        let name_len = read_u16(bytes, base + PERF_METRIC_NAME_LEN_OFFSET)? as usize;
        let name_offset = read_u32(bytes, base + PERF_METRIC_NAME_OFFSET_OFFSET)? as usize;
        if value_offset
            .checked_add(metric_value_count)
            .ok_or(EINVAL)?
            > value_count
            || name_offset.checked_add(name_len).ok_or(EINVAL)? > names.len()
            || (kind == PerfMetricKind::Counter && metric_value_count != 1)
            || (kind == PerfMetricKind::Histogram
                && metric_value_count != histogram_bucket_count)
        {
            return Err(EINVAL);
        }
        let name = core::str::from_utf8(&names[name_offset..name_offset + name_len])
            .map_err(|_| EINVAL)?
            .into();
        if metrics
            .iter()
            .any(|metric: &PerfMetricDescriptor| metric.name == name)
        {
            return Err(EINVAL);
        }
        metrics.push(PerfMetricDescriptor {
            id: read_u32(bytes, base + PERF_METRIC_ID_OFFSET)?,
            kind,
            unit,
            value_offset,
            value_count: metric_value_count,
            name,
        });
    }
    Ok(PerfCatalog {
        clock_frequency_hz: read_u64(bytes, PERF_CATALOG_CLOCK_FREQUENCY_HZ_OFFSET)?,
        histogram_bucket_count,
        value_count,
        metrics,
    })
}

fn parse_snapshot(bytes: &[u8]) -> Result<PerfSnapshot, Errno> {
    if bytes.len() < PERF_SNAPSHOT_HEADER_SIZE
        || bytes[PERF_SNAPSHOT_RESERVED_OFFSET..PERF_SNAPSHOT_HEADER_SIZE]
            .iter()
            .any(|byte| *byte != 0)
    {
        return Err(EINVAL);
    }
    let value_count = read_u32(bytes, PERF_SNAPSHOT_VALUE_COUNT_OFFSET)? as usize;
    let expected = PERF_SNAPSHOT_HEADER_SIZE
        .checked_add(value_count.checked_mul(size_of::<u64>()).ok_or(EINVAL)?)
        .ok_or(EINVAL)?;
    if bytes.len() != expected {
        return Err(EINVAL);
    }
    let enabled = match bytes[PERF_SNAPSHOT_ENABLED_OFFSET] {
        0 => false,
        1 => true,
        _ => return Err(EINVAL),
    };
    let begin_ticks = read_u64(bytes, PERF_SNAPSHOT_BEGIN_TICKS_OFFSET)?;
    let end_ticks = read_u64(bytes, PERF_SNAPSHOT_END_TICKS_OFFSET)?;
    if end_ticks < begin_ticks {
        return Err(EINVAL);
    }
    let mut values = Vec::with_capacity(value_count);
    for index in 0..value_count {
        values.push(read_u64(
            bytes,
            PERF_SNAPSHOT_HEADER_SIZE + index * size_of::<u64>(),
        )?);
    }
    Ok(PerfSnapshot {
        begin_ticks,
        end_ticks,
        enabled,
        values,
    })
}
