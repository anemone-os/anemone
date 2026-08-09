use anemone_abi::system::native::perf::*;

use crate::{
    debug::perf::{self, MetricRegistration},
    prelude::{
        user_access::{UserWriteSlice, user_addr},
        *,
    },
    task::credentials::cap::Capability,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Request {
    Query { required: usize, size_only: bool },
    GetEnabled,
    SetEnabled(bool),
    Snapshot { required: usize, size_only: bool },
}

fn validate_request(
    op: u64,
    arg: u64,
    buf: u64,
    len: usize,
    flags: u64,
    catalog_len: usize,
    snapshot_len: usize,
) -> Result<Request, SysError> {
    if flags != 0 {
        return Err(SysError::InvalidArgument);
    }
    match op {
        PERF_OBSERVE_QUERY => {
            if arg != 0 {
                return Err(SysError::InvalidArgument);
            }
            validate_output(buf, len, catalog_len).map(|size_only| Request::Query {
                required: catalog_len,
                size_only,
            })
        },
        PERF_OBSERVE_GET_ENABLED => {
            if arg != 0 || buf != 0 || len != 0 {
                return Err(SysError::InvalidArgument);
            }
            Ok(Request::GetEnabled)
        },
        PERF_OBSERVE_SET_ENABLED => {
            if arg > 1 || buf != 0 || len != 0 {
                return Err(SysError::InvalidArgument);
            }
            Ok(Request::SetEnabled(arg != 0))
        },
        PERF_OBSERVE_SNAPSHOT => {
            if arg != 0 {
                return Err(SysError::InvalidArgument);
            }
            validate_output(buf, len, snapshot_len).map(|size_only| Request::Snapshot {
                required: snapshot_len,
                size_only,
            })
        },
        _ => Err(SysError::InvalidArgument),
    }
}

fn validate_output(buf: u64, len: usize, required: usize) -> Result<bool, SysError> {
    if buf == 0 && len == 0 {
        return Ok(true);
    }
    if len < required {
        return Err(SysError::NoSpace);
    }
    Ok(false)
}

fn copyout_at(
    usp: &mut UserSpaceGuard<'_>,
    base: u64,
    offset: usize,
    bytes: &[u8],
) -> Result<(), SysError> {
    let address = base
        .checked_add(offset as u64)
        .ok_or(SysError::BadAddress)?;
    let mut user = UserWriteSlice::<u8>::try_new(user_addr(address)?, bytes.len(), usp)?;
    user.copy_from_slice(bytes)
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + size_of::<u16>()].copy_from_slice(&value.to_ne_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + size_of::<u32>()].copy_from_slice(&value.to_ne_bytes());
}

fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + size_of::<u64>()].copy_from_slice(&value.to_ne_bytes());
}

fn encode_catalog_header() -> [u8; PERF_CATALOG_HEADER_SIZE] {
    let layout = perf::catalog_layout();
    let mut bytes = [0u8; PERF_CATALOG_HEADER_SIZE];
    put_u32(
        &mut bytes,
        PERF_CATALOG_CLOCK_KIND_OFFSET,
        PERF_CLOCK_MONOTONIC_RAW,
    );
    put_u32(
        &mut bytes,
        PERF_CATALOG_METRIC_COUNT_OFFSET,
        layout.metric_count.try_into().unwrap(),
    );
    put_u32(
        &mut bytes,
        PERF_CATALOG_VALUE_COUNT_OFFSET,
        layout.value_count.try_into().unwrap(),
    );
    put_u32(
        &mut bytes,
        PERF_CATALOG_HISTOGRAM_BUCKET_COUNT_OFFSET,
        PERF_HISTOGRAM_BUCKET_COUNT.try_into().unwrap(),
    );
    put_u64(
        &mut bytes,
        PERF_CATALOG_CLOCK_FREQUENCY_HZ_OFFSET,
        crate::time::perf_clock_frequency_hz(),
    );
    put_u32(
        &mut bytes,
        PERF_CATALOG_NAME_BYTES_OFFSET,
        layout.name_bytes.try_into().unwrap(),
    );
    bytes
}

fn encode_descriptor(
    id: usize,
    value_offset: usize,
    name_offset: usize,
    metric: &MetricRegistration,
) -> [u8; PERF_METRIC_DESCRIPTOR_SIZE] {
    let mut bytes = [0u8; PERF_METRIC_DESCRIPTOR_SIZE];
    put_u32(&mut bytes, PERF_METRIC_ID_OFFSET, id.try_into().unwrap());
    put_u16(&mut bytes, PERF_METRIC_KIND_OFFSET, metric.kind() as u16);
    put_u16(&mut bytes, PERF_METRIC_UNIT_OFFSET, metric.unit() as u16);
    put_u32(
        &mut bytes,
        PERF_METRIC_VALUE_OFFSET_OFFSET,
        value_offset.try_into().unwrap(),
    );
    put_u16(
        &mut bytes,
        PERF_METRIC_VALUE_COUNT_OFFSET,
        metric.value_count().try_into().unwrap(),
    );
    put_u16(
        &mut bytes,
        PERF_METRIC_NAME_LEN_OFFSET,
        metric.name().len().try_into().unwrap(),
    );
    put_u32(
        &mut bytes,
        PERF_METRIC_NAME_OFFSET_OFFSET,
        name_offset.try_into().unwrap(),
    );
    bytes
}

fn write_catalog(usp: &mut UserSpaceGuard<'_>, base: u64) -> Result<(), SysError> {
    let layout = perf::catalog_layout();
    copyout_at(usp, base, 0, &encode_catalog_header())?;
    let descriptors_base = PERF_CATALOG_HEADER_SIZE;
    let names_base = descriptors_base + PERF_METRIC_DESCRIPTOR_SIZE * layout.metric_count;
    let mut name_offset = 0;
    perf::try_for_each_metric(|id, value_offset, metric| {
        let descriptor = encode_descriptor(id, value_offset, name_offset, metric);
        copyout_at(
            usp,
            base,
            descriptors_base + id * PERF_METRIC_DESCRIPTOR_SIZE,
            &descriptor,
        )?;
        copyout_at(
            usp,
            base,
            names_base + name_offset,
            metric.name().as_bytes(),
        )?;
        name_offset += metric.name().len();
        Ok::<_, SysError>(())
    })?;
    assert_eq!(name_offset, layout.name_bytes);
    Ok(())
}

fn encode_snapshot_header(
    begin_ticks: u64,
    end_ticks: u64,
    value_count: usize,
    enabled: bool,
) -> [u8; PERF_SNAPSHOT_HEADER_SIZE] {
    let mut bytes = [0u8; PERF_SNAPSHOT_HEADER_SIZE];
    put_u64(&mut bytes, PERF_SNAPSHOT_BEGIN_TICKS_OFFSET, begin_ticks);
    put_u64(&mut bytes, PERF_SNAPSHOT_END_TICKS_OFFSET, end_ticks);
    put_u32(
        &mut bytes,
        PERF_SNAPSHOT_VALUE_COUNT_OFFSET,
        value_count.try_into().unwrap(),
    );
    bytes[PERF_SNAPSHOT_ENABLED_OFFSET] = u8::from(enabled);
    bytes
}

fn write_snapshot(usp: &mut UserSpaceGuard<'_>, base: u64) -> Result<(), SysError> {
    let layout = perf::catalog_layout();
    let begin_ticks = crate::time::perf_clock_ticks();
    let enabled = perf::recording_enabled();
    let mut index = 0usize;
    perf::snapshot_values(|value| {
        let offset = PERF_SNAPSHOT_HEADER_SIZE + index * size_of::<u64>();
        copyout_at(usp, base, offset, &value.to_ne_bytes())?;
        index += 1;
        Ok::<_, SysError>(())
    })?;
    assert_eq!(index, layout.value_count);
    let end_ticks = crate::time::perf_clock_ticks();
    let header = encode_snapshot_header(begin_ticks, end_ticks, layout.value_count, enabled);
    // The header is published last. A fault may leave a partial values area,
    // but no failed call exposes a newly written success-looking header.
    copyout_at(usp, base, 0, &header)
}

/// Native developer ABI for catalog discovery, recording control and snapshots.
/// Validation and size errors precede capability checks; protected operation
/// copyout begins only after effective `CAP_SYS_ADMIN` admission.
// The observation control plane must not recursively contaminate the metrics
// whose snapshots it publishes.
#[syscall(SYS_PERF_OBSERVE, profile = false)]
fn sys_perf_observe(op: u64, arg: u64, buf: u64, len: usize, flags: u64) -> Result<u64, SysError> {
    let layout = perf::catalog_layout();
    let snapshot_len = PERF_SNAPSHOT_HEADER_SIZE
        .checked_add(
            layout
                .value_count
                .checked_mul(size_of::<u64>())
                .expect("performance snapshot value bytes overflow"),
        )
        .expect("performance snapshot bytes overflow");
    let request = validate_request(op, arg, buf, len, flags, layout.byte_len, snapshot_len)?;
    let has_sys_admin = get_current_task().has_cap(Capability::SYS_ADMIN);

    match request {
        Request::Query {
            required,
            size_only,
        } => {
            if !size_only {
                get_current_task()
                    .clone_uspace_handle()
                    .with_usp(|usp| write_catalog(usp, buf))?;
            }
            Ok(required as u64)
        },
        Request::GetEnabled => Ok(u64::from(perf::recording_enabled())),
        Request::SetEnabled(enabled) => {
            if !has_sys_admin {
                return Err(SysError::PermissionDenied);
            }
            Ok(u64::from(perf::replace_recording_enabled(enabled)))
        },
        Request::Snapshot {
            required,
            size_only,
        } => {
            if !has_sys_admin {
                return Err(SysError::PermissionDenied);
            }
            if !size_only {
                get_current_task()
                    .clone_uspace_handle()
                    .with_usp(|usp| write_snapshot(usp, buf))?;
            }
            Ok(required as u64)
        },
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn request_validation_freezes_structure_and_size_precedence() {
        let catalog = 128;
        let snapshot = 256;
        assert_eq!(
            validate_request(PERF_OBSERVE_QUERY, 0, 0, 0, 0, catalog, snapshot),
            Ok(Request::Query {
                required: catalog,
                size_only: true
            })
        );
        assert_eq!(
            validate_request(
                PERF_OBSERVE_SNAPSHOT,
                0,
                1,
                snapshot - 1,
                0,
                catalog,
                snapshot
            ),
            Err(SysError::NoSpace)
        );
        assert_eq!(
            validate_request(PERF_OBSERVE_SET_ENABLED, 2, 0, 0, 0, catalog, snapshot),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(
            validate_request(PERF_OBSERVE_GET_ENABLED, 0, 1, 0, 0, catalog, snapshot),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(
            validate_request(PERF_OBSERVE_QUERY, 0, 0, 0, 1, catalog, snapshot),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(
            validate_request(u64::MAX, 0, 0, 0, 0, catalog, snapshot),
            Err(SysError::InvalidArgument)
        );
    }

    #[kunit]
    fn codecs_initialize_every_wire_byte_and_preserve_layout() {
        let catalog = encode_catalog_header();
        assert_eq!(
            u32::from_ne_bytes(
                catalog[PERF_CATALOG_RESERVED_OFFSET..][..4]
                    .try_into()
                    .unwrap()
            ),
            0
        );
        let snapshot = encode_snapshot_header(11, 22, 3, true);
        assert_eq!(snapshot[PERF_SNAPSHOT_ENABLED_OFFSET], 1);
        assert_eq!(
            &snapshot[PERF_SNAPSHOT_RESERVED_OFFSET..PERF_SNAPSHOT_HEADER_SIZE],
            &[0, 0, 0]
        );
    }

    #[kunit]
    fn set_returns_complete_old_gate_and_rejected_validation_does_not_mutate() {
        let old = perf::replace_recording_enabled(false);
        assert!(!perf::replace_recording_enabled(true));
        let rejected = validate_request(
            PERF_OBSERVE_SET_ENABLED,
            2,
            0,
            0,
            0,
            perf::catalog_layout().byte_len,
            PERF_SNAPSHOT_HEADER_SIZE,
        );
        assert_eq!(rejected, Err(SysError::InvalidArgument));
        assert!(perf::recording_enabled());
        assert!(perf::replace_recording_enabled(old));
    }

    #[kunit]
    fn descriptor_codec_uses_kind_unit_offsets_and_zero_reserved() {
        let mut checked = 0;
        perf::try_for_each_metric::<()>(|id, value_offset, metric| {
            let descriptor = encode_descriptor(id, value_offset, 7, metric);
            assert_eq!(
                u32::from_ne_bytes(
                    descriptor[PERF_METRIC_RESERVED_OFFSET..][..4]
                        .try_into()
                        .unwrap()
                ),
                0
            );
            let kind = u16::from_ne_bytes(
                descriptor[PERF_METRIC_KIND_OFFSET..][..2]
                    .try_into()
                    .unwrap(),
            );
            assert!(matches!(
                kind,
                PERF_METRIC_COUNTER | PERF_METRIC_HISTOGRAM | PERF_METRIC_ELAPSED
            ));
            let unit = u16::from_ne_bytes(
                descriptor[PERF_METRIC_UNIT_OFFSET..][..2]
                    .try_into()
                    .unwrap(),
            );
            assert!(matches!(unit, PERF_UNIT_EVENTS | PERF_UNIT_MONOTONIC_TICKS));
            checked += 1;
            Ok(())
        })
        .unwrap();
        assert!(checked > 0);
    }
}
