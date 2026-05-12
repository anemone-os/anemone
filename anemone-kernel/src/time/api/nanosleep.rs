//! nanosleep syscall implementation.
//!
//! TODO: get this working correctly when signal is implemented.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/nanosleep.2.html

use anemone_abi::time::linux::TimeSpec;

use crate::prelude::{
    user_access::{SyscallArgValidatorExt, UserReadPtr, user_addr},
    *,
};

// see man 2 nanosleep for this.
const TV_NSEC_MAX_INCLUSIVE: u64 = 999_999_999;

fn validate_time_spec(ts: &TimeSpec) -> Result<(), SysError> {
    if ts.tv_nsec < 0 || ts.tv_sec < 0 {
        return Err(SysError::InvalidArgument);
    }
    Ok(())
}

#[syscall(SYS_NANOSLEEP)]
fn sys_nanosleep(
    #[validate_with(user_addr)] duration: VirtAddr,
    // currently unused, since we haven't implemented signal handling yet.
    #[validate_with(user_addr.nullable())] _rem: Option<VirtAddr>,
    // _rem: Option<UserReadPtr<TimeSpec>>,
) -> Result<u64, SysError> {
    let usp = get_current_task().clone_uspace_handle();
    let mut guard = usp.lock();
    let duration = UserReadPtr::<TimeSpec>::try_new(duration, &mut guard)?.read();

    validate_time_spec(&duration)?;
    if duration.tv_nsec as u64 > TV_NSEC_MAX_INCLUSIVE {
        return Err(SysError::InvalidArgument);
    }

    // we don't have a strong enough wait queue.
    // currently just use a simple loop + yield instead.
    let duration2wait = Duration::new(
        duration.tv_sec as u64,
        // this will not overflow since we checked above that tv_nsec is less than 1e9, which fits
        // in u32.
        duration.tv_nsec as u32,
    );

    let now = uptime();
    loop {
        let cur = uptime();
        if cur >= now + duration2wait {
            break;
        }
        yield_now();
    }

    Ok(0)
}
