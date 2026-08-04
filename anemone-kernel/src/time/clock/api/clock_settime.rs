//! `clock_settime` system call.

use anemone_abi::{
    syscall::SYS_CLOCK_SETTIME,
    time::linux::{TimeSpec, clock::CLOCK_REALTIME},
};

use crate::{
    prelude::*,
    syscall::user_access::{UserReadPtr, user_addr},
    task::credentials::cap::Capability,
};

use super::{publish_realtime_step, timespec_to_ns};

#[syscall(SYS_CLOCK_SETTIME)]
fn sys_clock_settime(
    which_clock: i32,
    #[validate_with(user_addr)] tp: VirtAddr,
) -> Result<u64, SysError> {
    if which_clock != CLOCK_REALTIME {
        return Err(SysError::InvalidArgument);
    }

    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let target_ns = {
        let mut usp = uspace.lock();
        timespec_to_ns(UserReadPtr::<TimeSpec>::try_new(tp, &mut usp)?.read()?)?
    };
    if !task.has_cap(Capability::SYS_TIME) {
        return Err(SysError::PermissionDenied);
    }

    publish_realtime_step(set_realtime_ns(target_ns)?);
    Ok(0)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn settime_timespec_validation_rejects_invalid_and_overflowing_values() {
        assert_eq!(
            timespec_to_ns(TimeSpec {
                tv_sec: -1,
                tv_nsec: 0,
            }),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(
            timespec_to_ns(TimeSpec {
                tv_sec: 0,
                tv_nsec: 1_000_000_000,
            }),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(
            timespec_to_ns(TimeSpec {
                tv_sec: i64::MAX,
                tv_nsec: 0,
            }),
            Err(SysError::InvalidArgument)
        );
    }
}
