//! Linux network-interface queries shared by all common Socket files.

use crate::{
    fs::IoctlCtx,
    net::route_diagnostics,
    prelude::*,
    syscall::user_access::{UserReadSlice, UserWritePtr, UserWriteSlice},
};

use anemone_net_api::Ipv4Address;

/// Linux 6.6.32 `SIOCGIFCONF` from `include/uapi/linux/sockios.h`.
pub(super) const SIOCGIFCONF: u32 = 0x8912;

const IFNAMSIZ: usize = 16;
const IFCONF_LEN: usize = 16;
const IFCONF_LENGTH_OFFSET: usize = 0;
const IFCONF_BUFFER_OFFSET: usize = 8;
const IFREQ_LEN: usize = 40;
const IFREQ_ADDRESS_OFFSET: usize = IFNAMSIZ;
const SOCKADDR_IN_ADDRESS_OFFSET: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IfConfInput {
    requested_len: i32,
    buffer: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IfConfPlan {
    record_count: usize,
    reported_len: i32,
}

fn parse_ifconf(bytes: [u8; IFCONF_LEN]) -> IfConfInput {
    let requested_len = i32::from_ne_bytes(
        bytes[IFCONF_LENGTH_OFFSET..IFCONF_LENGTH_OFFSET + size_of::<i32>()]
            .try_into()
            .unwrap(),
    );
    let buffer = u64::from_ne_bytes(
        bytes[IFCONF_BUFFER_OFFSET..IFCONF_BUFFER_OFFSET + size_of::<u64>()]
            .try_into()
            .unwrap(),
    );
    IfConfInput {
        requested_len,
        buffer,
    }
}

fn plan_ifconf(record_count: usize, input: IfConfInput) -> Result<IfConfPlan, SysError> {
    let total_len = record_count
        .checked_mul(IFREQ_LEN)
        .ok_or(SysError::FileTooLarge)?;
    let total_len = i32::try_from(total_len).map_err(|_| SysError::FileTooLarge)?;

    if input.buffer == 0 {
        return Ok(IfConfPlan {
            record_count: 0,
            reported_len: total_len,
        });
    }

    // Linux treats a negative non-NULL capacity like a short buffer: no
    // records fit, and the returned length is zero rather than EINVAL.
    let capacity = usize::try_from(input.requested_len).unwrap_or(0);
    let record_count = record_count.min(capacity / IFREQ_LEN);
    let reported_len = i32::try_from(record_count * IFREQ_LEN)
        .expect("record count was bounded by an i32-sized complete snapshot");
    Ok(IfConfPlan {
        record_count,
        reported_len,
    })
}

fn encode_ifreq(label: &str, address: Ipv4Address) -> [u8; IFREQ_LEN] {
    let mut record = [0u8; IFREQ_LEN];
    let label = label.as_bytes();
    // The current logical owner publishes only `lo` and `eth<u32>`, both of
    // which fit Linux IFNAMSIZ. Keep a future naming change explicit rather
    // than silently truncating two identities to the same ABI name.
    assert!(
        label.len() < IFNAMSIZ,
        "published logical-interface name does not fit Linux IFNAMSIZ"
    );
    record[..label.len()].copy_from_slice(label);

    record[IFREQ_ADDRESS_OFFSET..IFREQ_ADDRESS_OFFSET + size_of::<u16>()]
        .copy_from_slice(&(anemone_abi::net::linux::AF_INET as u16).to_ne_bytes());
    let address_offset = IFREQ_ADDRESS_OFFSET + SOCKADDR_IN_ADDRESS_OFFSET;
    record[address_offset..address_offset + 4].copy_from_slice(&address.octets());
    record
}

pub(super) fn get_interface_configuration(ctx: &IoctlCtx<'_>) -> Result<u64, SysError> {
    let mut header = [0u8; IFCONF_LEN];
    ctx.uspace().with_usp(|usp| {
        UserReadSlice::<u8>::try_new(VirtAddr::new(ctx.arg()), header.len(), usp)?
            .copy_to_slice(&mut header)
    })?;
    let input = parse_ifconf(header);

    // `route_diagnostics()` returns an owned request-local snapshot. No
    // network owner guard is held while either nested userspace copy runs.
    let diagnostics = route_diagnostics();
    let plan = plan_ifconf(diagnostics.addresses.len(), input)?;
    if plan.record_count != 0 {
        let mut records = Vec::with_capacity(plan.record_count * IFREQ_LEN);
        for address in &diagnostics.addresses[..plan.record_count] {
            records.extend_from_slice(&encode_ifreq(&address.label, address.address));
        }
        ctx.uspace().with_usp(|usp| {
            UserWriteSlice::<u8>::try_new(VirtAddr::new(input.buffer), records.len(), usp)?
                .copy_from_slice(&records)
        })?;
    }

    // Linux exposes successfully copied records before updating `ifc_len`.
    // A fault here must not roll back or retry the nested-buffer copy.
    ctx.uspace().with_usp(|usp| {
        UserWritePtr::<i32>::try_new(VirtAddr::new(ctx.arg()), usp)?.write(plan.reported_len)
    })?;
    Ok(0)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn input(requested_len: i32, buffer: u64) -> IfConfInput {
        IfConfInput {
            requested_len,
            buffer,
        }
    }

    #[kunit]
    fn lp64_ifconf_and_ifreq_layout_matches_linux() {
        assert_eq!(IFCONF_LEN, 16);
        assert_eq!(IFCONF_LENGTH_OFFSET, 0);
        assert_eq!(IFCONF_BUFFER_OFFSET, 8);
        assert_eq!(IFREQ_LEN, 40);
        assert_eq!(IFREQ_ADDRESS_OFFSET, 16);

        let mut bytes = [0xa5; IFCONF_LEN];
        bytes[..4].copy_from_slice(&80i32.to_ne_bytes());
        bytes[8..16].copy_from_slice(&0x1122_3344_5566_7788u64.to_ne_bytes());
        assert_eq!(parse_ifconf(bytes), input(80, 0x1122_3344_5566_7788));
    }

    #[kunit]
    fn ifreq_encodes_zeroed_loopback_and_external_ipv4_records() {
        let record = encode_ifreq("lo", Ipv4Address::new([127, 0, 0, 1]));

        assert_eq!(&record[..3], b"lo\0");
        assert!(record[3..IFNAMSIZ].iter().all(|byte| *byte == 0));
        assert_eq!(
            &record[IFREQ_ADDRESS_OFFSET..IFREQ_ADDRESS_OFFSET + 2],
            &(anemone_abi::net::linux::AF_INET as u16).to_ne_bytes()
        );
        assert_eq!(&record[18..20], &[0, 0]);
        assert_eq!(&record[20..24], &[127, 0, 0, 1]);
        assert!(record[24..].iter().all(|byte| *byte == 0));

        let record = encode_ifreq("eth0", Ipv4Address::new([10, 0, 2, 15]));
        assert_eq!(&record[..5], b"eth0\0");
        assert!(record[5..IFNAMSIZ].iter().all(|byte| *byte == 0));
        assert_eq!(&record[20..24], &[10, 0, 2, 15]);
        assert!(record[24..].iter().all(|byte| *byte == 0));
    }

    #[kunit]
    fn ifconf_null_buffer_reports_complete_snapshot_size() {
        assert_eq!(
            plan_ifconf(2, input(0, 0)),
            Ok(IfConfPlan {
                record_count: 0,
                reported_len: 2 * IFREQ_LEN as i32,
            })
        );
        assert_eq!(
            plan_ifconf(2, input(-1, 0)),
            Ok(IfConfPlan {
                record_count: 0,
                reported_len: 2 * IFREQ_LEN as i32,
            })
        );
    }

    #[kunit]
    fn ifconf_capacity_never_selects_a_partial_record() {
        for (capacity, records) in [
            (-1, 0),
            (0, 0),
            (IFREQ_LEN as i32 - 1, 0),
            (IFREQ_LEN as i32, 1),
            (IFREQ_LEN as i32 + 1, 1),
            (2 * IFREQ_LEN as i32, 2),
            (3 * IFREQ_LEN as i32, 2),
        ] {
            assert_eq!(
                plan_ifconf(2, input(capacity, 1)),
                Ok(IfConfPlan {
                    record_count: records,
                    reported_len: (records * IFREQ_LEN) as i32,
                })
            );
        }
    }

    #[kunit]
    fn ifconf_rejects_unrepresentable_snapshot_lengths() {
        let too_many = i32::MAX as usize / IFREQ_LEN + 1;
        assert_eq!(
            plan_ifconf(too_many, input(i32::MAX, 1)),
            Err(SysError::FileTooLarge)
        );
        assert_eq!(
            plan_ifconf(usize::MAX, input(i32::MAX, 1)),
            Err(SysError::FileTooLarge)
        );
    }
}
