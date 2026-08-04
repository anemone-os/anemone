//! `clock_nanosleep` system call.

use anemone_abi::time::linux::{TimeSpec, clock::TIMER_ABSTIME};

use crate::{
    prelude::*,
    syscall::user_access::{SyscallArgValidatorExt as _, UserReadPtr, UserWritePtr, user_addr},
    time::{
        clock::{SleepClock, get_sleep_clock},
        timer::schedule_realtime_threaded_timer_event,
    },
};

use super::{ns_to_duration, ns_to_timespec, timespec_to_ns};

static IGNORED_FLAGS_LOGGED: AtomicBool = AtomicBool::new(false);

#[syscall(SYS_CLOCK_NANOSLEEP)]
fn sys_clock_nanosleep(
    which_clock: i32,
    flags: i32,
    #[validate_with(user_addr)] rqtp: VirtAddr,
    #[validate_with(user_addr.nullable())] rmtp: Option<VirtAddr>,
) -> Result<u64, SysError> {
    clock_nanosleep(which_clock, flags, rqtp, rmtp)
}

pub(crate) fn clock_nanosleep(
    which_clock: i32,
    flags: i32,
    rqtp: VirtAddr,
    rmtp: Option<VirtAddr>,
) -> Result<u64, SysError> {
    let clock = get_sleep_clock(which_clock)?;
    let ignored_flags = flags & !TIMER_ABSTIME;
    if ignored_flags != 0 && !IGNORED_FLAGS_LOGGED.swap(true, Ordering::Relaxed) {
        // Linux's legacy clock_nanosleep ABI ignores every flag except
        // TIMER_ABSTIME. Keep this compatibility behavior observable without
        // turning unknown bits into EINVAL.
        knoticeln!(
            "clock_nanosleep: ignoring legacy flag bits {:#x}",
            ignored_flags,
        );
    }

    let task = get_current_task();
    let usp_handle = task.clone_uspace_handle();
    let requested_ns = {
        let mut usp = usp_handle.lock();
        timespec_to_ns(UserReadPtr::<TimeSpec>::try_new(rqtp, &mut usp)?.read()?)?
    };
    let absolute = flags & TIMER_ABSTIME != 0;

    if !absolute {
        return sleep_relative(&task, ns_to_duration(requested_ns), rmtp);
    }
    match clock {
        SleepClock::Monotonic => sleep_absolute_monotonic(&task, requested_ns),
        SleepClock::Realtime => sleep_absolute_realtime(&task, requested_ns),
    }
}

fn sleep_relative(
    task: &Arc<Task>,
    duration: Duration,
    rmtp: Option<VirtAddr>,
) -> Result<u64, SysError> {
    let duration_ns = u64::try_from(duration.as_nanos()).map_err(|_| SysError::InvalidArgument)?;
    let deadline_ns = monotonic_ns()
        .checked_add(duration_ns)
        .ok_or(SysError::InvalidArgument)?;
    loop {
        let now_ns = monotonic_ns();
        if now_ns >= deadline_ns {
            return Ok(0);
        }
        let rem = ns_to_duration(deadline_ns - now_ns);
        let (outcome, _) = wait_current_with_timeout(task, true, Some(rem), || {
            task.has_unmasked_signal()
                .then_some(CurrentWaitPrecheck::Signal)
        });
        match outcome {
            CurrentWaitOutcome::Timeout => continue,
            CurrentWaitOutcome::Signal | CurrentWaitOutcome::Force => {
                write_remaining_time(
                    rmtp,
                    ns_to_duration(deadline_ns.saturating_sub(monotonic_ns())),
                )?;
                return Err(SysError::Interrupted);
            },
            other => panic!("relative clock_nanosleep saw unexpected wait outcome {other:?}"),
        }
    }
}

fn sleep_absolute_monotonic(task: &Arc<Task>, deadline_ns: u64) -> Result<u64, SysError> {
    loop {
        let now_ns = monotonic_ns();
        if now_ns >= deadline_ns {
            return Ok(0);
        }
        let timeout = ns_to_duration(deadline_ns - now_ns);
        let (outcome, _) = wait_current_with_timeout(task, true, Some(timeout), || {
            task.has_unmasked_signal()
                .then_some(CurrentWaitPrecheck::Signal)
        });
        match outcome {
            CurrentWaitOutcome::Timeout => continue,
            CurrentWaitOutcome::Signal | CurrentWaitOutcome::Force => {
                return Err(SysError::Interrupted);
            },
            other => panic!("absolute monotonic sleep saw unexpected wait outcome {other:?}"),
        }
    }
}

fn sleep_absolute_realtime(task: &Arc<Task>, deadline_ns: u64) -> Result<u64, SysError> {
    if realtime_ns() >= deadline_ns {
        return Ok(0);
    }
    let outcome = wait_current_with_timer_request(
        task,
        true,
        |trigger| {
            schedule_realtime_threaded_timer_event(
                deadline_ns,
                None,
                Box::new(move || trigger.expire()),
                None,
            )
        },
        || {
            task.has_unmasked_signal()
                .then_some(CurrentWaitPrecheck::Signal)
        },
    );
    match outcome {
        CurrentWaitOutcome::Timeout => Ok(0),
        CurrentWaitOutcome::Signal | CurrentWaitOutcome::Force => Err(SysError::Interrupted),
        other => panic!("absolute realtime sleep saw unexpected wait outcome {other:?}"),
    }
}

fn write_remaining_time(rmtp: Option<VirtAddr>, rem: Duration) -> Result<(), SysError> {
    let Some(rmtp) = rmtp else {
        return Ok(());
    };
    let task = get_current_task();
    let usp_handle = task.clone_uspace_handle();
    let mut usp = usp_handle.lock();
    UserWritePtr::<TimeSpec>::try_new(rmtp, &mut usp)?.write(ns_to_timespec(
        u64::try_from(rem.as_nanos()).expect("remaining sleep exceeds native timespec range"),
    ))?;
    Ok(())
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn sleep_clock_operation_matrix_is_explicit() {
        assert_eq!(get_sleep_clock(0), Ok(SleepClock::Realtime));
        assert_eq!(get_sleep_clock(1), Ok(SleepClock::Monotonic));
        assert_eq!(get_sleep_clock(7), Ok(SleepClock::Monotonic));
        assert_eq!(get_sleep_clock(2), Err(SysError::NotSupported));
        assert_eq!(get_sleep_clock(4), Err(SysError::InvalidArgument));
        assert_eq!(get_sleep_clock(8), Err(SysError::InvalidArgument));
    }
}
