//! `clock_adjtime` system call.

use anemone_abi::{
    syscall::SYS_CLOCK_ADJTIME,
    time::linux::{
        Timex,
        clock::CLOCK_REALTIME,
        timex::{
            ADJ_ESTERROR, ADJ_FREQUENCY, ADJ_MAXERROR, ADJ_MICRO, ADJ_NANO, ADJ_OFFSET,
            ADJ_SETOFFSET, ADJ_STATUS, ADJ_TAI, ADJ_TICK, ADJ_TIMECONST, STA_UNSYNC, TIME_ERROR,
        },
    },
};

use crate::{
    prelude::*,
    syscall::user_access::{UserReadPtr, UserWritePtr, user_addr},
    task::credentials::cap::Capability,
};

use super::{ns_to_timeval, publish_realtime_step};

const ADJ_SINGLESHOT: u32 = 0x8000;
// Keep the Linux distinction between malformed input and a recognized feature
// outside this stage. Unknown or contradictory bits are EINVAL; a known but
// unimplemented discipline operation is observable as EOPNOTSUPP.
const KNOWN_MODE_BITS: u32 = ADJ_OFFSET
    | ADJ_FREQUENCY
    | ADJ_MAXERROR
    | ADJ_ESTERROR
    | ADJ_STATUS
    | ADJ_TIMECONST
    | ADJ_TAI
    | ADJ_SETOFFSET
    | ADJ_MICRO
    | ADJ_NANO
    | ADJ_TICK
    | ADJ_SINGLESHOT;
const SUPPORTED_MODE_BITS: u32 = ADJ_SETOFFSET | ADJ_MICRO | ADJ_NANO;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdjOperation {
    Query,
    SetOffset(i128),
}

fn decode_operation(tx: Timex) -> Result<AdjOperation, SysError> {
    let modes = tx.modes;
    if modes == 0 {
        return Ok(AdjOperation::Query);
    }
    if modes & !KNOWN_MODE_BITS != 0 {
        return Err(SysError::InvalidArgument);
    }
    if modes & (ADJ_MICRO | ADJ_NANO) == (ADJ_MICRO | ADJ_NANO)
        || modes & ADJ_SETOFFSET == 0 && modes & (ADJ_MICRO | ADJ_NANO) != 0
    {
        return Err(SysError::InvalidArgument);
    }
    if modes & !SUPPORTED_MODE_BITS != 0 {
        knoticeln!(
            "clock_adjtime: rejecting unsupported adjustment modes {:#x}",
            modes,
        );
        return Err(SysError::NotSupported);
    }
    if modes & ADJ_SETOFFSET == 0 {
        return Err(SysError::InvalidArgument);
    }

    let fractional_limit = if modes & ADJ_NANO != 0 {
        1_000_000_000_i64
    } else {
        1_000_000_i64
    };
    if tx.time.tv_usec < 0 || tx.time.tv_usec >= fractional_limit {
        return Err(SysError::InvalidArgument);
    }
    let fractional_scale = if modes & ADJ_NANO != 0 {
        1_i128
    } else {
        1_000_i128
    };
    // Linux ADJ_SETOFFSET represents a signed delta as signed seconds plus a
    // nonnegative fractional field. Thus {-1, 500ms} means -500ms, not -1.5s.
    // i128 keeps every native time64 input representable during normalization.
    let delta = i128::from(tx.time.tv_sec)
        .checked_mul(1_000_000_000)
        .and_then(|seconds| {
            i128::from(tx.time.tv_usec)
                .checked_mul(fractional_scale)
                .and_then(|fraction| seconds.checked_add(fraction))
        })
        .ok_or(SysError::InvalidArgument)?;
    Ok(AdjOperation::SetOffset(delta))
}

fn query_result(modes: u32) -> Timex {
    // R0 has no NTP/PLL discipline. Report the real source precision and
    // calendar value, but keep STA_UNSYNC/TIME_ERROR honest instead of filling
    // Linux discipline fields with invented state.
    Timex {
        modes,
        _padding0: 0,
        offset: 0,
        freq: 0,
        maxerror: 0,
        esterror: 0,
        status: STA_UNSYNC,
        _padding1: 0,
        constant: 0,
        precision: source_resolution_ns() as i64,
        tolerance: 0,
        time: ns_to_timeval(realtime_ns()),
        tick: 0,
        ppsfreq: 0,
        jitter: 0,
        shift: 0,
        _padding2: 0,
        stabil: 0,
        jitcnt: 0,
        calcnt: 0,
        errcnt: 0,
        stbcnt: 0,
        tai: 0,
        padding: [0; 11],
    }
}

#[syscall(SYS_CLOCK_ADJTIME)]
fn sys_clock_adjtime(
    which_clock: i32,
    #[validate_with(user_addr)] txp: VirtAddr,
) -> Result<u64, SysError> {
    if which_clock != CLOCK_REALTIME {
        return Err(SysError::InvalidArgument);
    }

    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let tx = {
        let mut usp = uspace.lock();
        UserReadPtr::<Timex>::try_new(txp, &mut usp)?.read()?
    };
    let operation = decode_operation(tx)?;
    // A successful mutation must not be followed by EFAULT while returning the
    // mandatory timex snapshot. Fault the output mapping before checking
    // privilege and before committing any realtime side effect.
    {
        let mut usp = uspace.lock();
        UserWritePtr::<Timex>::try_new(txp, &mut usp)?.fault_in()?;
    }
    if let AdjOperation::SetOffset(delta_ns) = operation {
        if !task.has_cap(Capability::SYS_TIME) {
            return Err(SysError::PermissionDenied);
        }
        publish_realtime_step(adjust_realtime_ns(delta_ns)?);
    }

    let output = query_result(tx.modes);
    let mut usp = uspace.lock();
    UserWritePtr::<Timex>::try_new(txp, &mut usp)?.write(output)?;
    // TIME_ERROR is the Linux clock state corresponding to STA_UNSYNC. It is a
    // successful syscall result, not an errno.
    Ok(TIME_ERROR as u64)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn adjtime_mode_matrix_distinguishes_invalid_unsupported_and_setoffset() {
        assert_eq!(decode_operation(Timex::default()), Ok(AdjOperation::Query));

        let mut tx = Timex {
            modes: ADJ_SETOFFSET | ADJ_NANO,
            ..Timex::default()
        };
        tx.time.tv_sec = -1;
        tx.time.tv_usec = 500_000_000;
        assert_eq!(
            decode_operation(tx),
            Ok(AdjOperation::SetOffset(-500_000_000))
        );

        tx.modes = ADJ_NANO;
        assert_eq!(decode_operation(tx), Err(SysError::InvalidArgument));
        tx.modes = ADJ_SETOFFSET | ADJ_NANO | ADJ_MICRO;
        assert_eq!(decode_operation(tx), Err(SysError::InvalidArgument));
        tx.modes = ADJ_FREQUENCY;
        assert_eq!(decode_operation(tx), Err(SysError::NotSupported));
        tx.modes = 0x0400;
        assert_eq!(decode_operation(tx), Err(SysError::InvalidArgument));
    }

    #[kunit]
    fn adjtime_query_reports_unsynchronized_real_precision() {
        let tx = query_result(0);
        assert_eq!(tx.status, STA_UNSYNC);
        assert_eq!(tx.precision, source_resolution_ns() as i64);
        assert_eq!(tx.offset, 0);
        assert_eq!(tx.freq, 0);
    }

    #[kunit]
    fn native_timex_layout_matches_asm_generic_time64() {
        assert_eq!(core::mem::size_of::<Timex>(), 208);
        assert_eq!(core::mem::align_of::<Timex>(), 8);
    }
}
