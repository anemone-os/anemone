//! clock_nanosleep system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/clock_nanosleep.2.html

use anemone_abi::time::linux::TimeSpec;

use crate::{
    prelude::*,
    syscall::user_access::{SyscallArgValidatorExt as _, UserReadPtr, UserWritePtr, user_addr},
};

fn timespec_to_duration(ts: TimeSpec) -> Result<Duration, SysError> {
    if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 {
        return Err(SysError::InvalidArgument);
    }

    Ok(Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32))
}

fn duration_to_timespec(duration: Duration) -> TimeSpec {
    TimeSpec {
        tv_sec: duration.as_secs() as i64,
        tv_nsec: duration.subsec_nanos() as i64,
    }
}

#[syscall(SYS_CLOCK_NANOSLEEP)]
fn sys_clock_nanosleep(
    which_clock: i32,
    flags: i32,
    #[validate_with(user_addr)] rqtp: VirtAddr,
    #[validate_with(user_addr.nullable())] rmtp: Option<VirtAddr>,
) -> Result<u64, SysError> {
    kdebugln!(
        "clock_nanosleep: which_clock={:#x}, flags={:#x}, rmtp={:?}",
        which_clock,
        flags,
        rmtp
    );

    clock_nanosleep(which_clock, flags, rqtp, rmtp)
}

pub(crate) fn clock_nanosleep(
    _which_clock: i32,
    _flags: i32,
    rqtp: VirtAddr,
    rmtp: Option<VirtAddr>,
) -> Result<u64, SysError> {
    let task = get_current_task();
    let usp_handle = task.clone_uspace_handle();
    let duration = {
        let mut usp = usp_handle.lock();
        timespec_to_duration(UserReadPtr::<TimeSpec>::try_new(rqtp, &mut usp)?.read())?
    };

    let mut rem = duration;
    while rem > Duration::ZERO {
        task.update_status_with(|_prev| {
            (
                TaskStatus::Waiting {
                    interruptible: true,
                },
                (),
            )
        });

        if task.has_unmasked_signal() {
            task.update_status_with(|_prev| (TaskStatus::Runnable, ()));
            write_remaining_time(rmtp, rem)?;
            return Err(SysError::Interrupted);
        }

        rem = schedule_with_timeout(Some(rem));

        if task.has_unmasked_signal() {
            write_remaining_time(rmtp, rem)?;
            return Err(SysError::Interrupted);
        }
    }

    Ok(0)
}

fn write_remaining_time(rmtp: Option<VirtAddr>, rem: Duration) -> Result<(), SysError> {
    let Some(rmtp) = rmtp else {
        return Ok(());
    };

    let task = get_current_task();
    let usp_handle = task.clone_uspace_handle();
    let mut usp = usp_handle.lock();
    UserWritePtr::<TimeSpec>::try_new(rmtp, &mut usp)?.write(duration_to_timespec(rem));
    Ok(())
}
