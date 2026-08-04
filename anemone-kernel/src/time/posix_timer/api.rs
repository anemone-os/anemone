use anemone_abi::time::linux::{
    ITimerSpec, SigEvent, TimeSpec,
    clock::{
        CLOCK_BOOTTIME, CLOCK_MONOTONIC, CLOCK_PROCESS_CPUTIME_ID, CLOCK_REALTIME,
        CLOCK_THREAD_CPUTIME_ID, TIMER_ABSTIME,
    },
    posix_timer::{SIGEV_NONE, SIGEV_SIGNAL},
};

use crate::{
    prelude::*,
    syscall::{
        handler::TryFromSyscallArg,
        user_access::{SyscallArgValidatorExt as _, UserReadPtr, UserWritePtr, user_addr},
    },
    task::{
        sig::SigNo,
        task_posix_timer::{PosixTimerClock, PosixTimerNotification, PosixTimerSetting},
    },
};

const NSEC_PER_SEC: u64 = 1_000_000_000;
static CPU_TIMER_UNSUPPORTED_LOGGED: AtomicBool = AtomicBool::new(false);
static NOTIFICATION_UNSUPPORTED_LOGGED: AtomicBool = AtomicBool::new(false);
static IGNORED_SETTIME_FLAGS_LOGGED: AtomicBool = AtomicBool::new(false);

#[syscall(SYS_TIMER_CREATE)]
fn sys_timer_create(
    clock_id: i32,
    event_ptr: u64,
    #[validate_with(user_addr)] timer_id_ptr: VirtAddr,
) -> Result<u64, SysError> {
    let clock = timer_clock(clock_id)?;
    let task = get_current_task();
    let notification = if event_ptr == 0 {
        PosixTimerNotification::DefaultSignal
    } else {
        let event = {
            let event_ptr = user_addr(event_ptr)?;
            let usp_handle = task.clone_uspace_handle();
            let mut usp = usp_handle.lock();
            UserReadPtr::<SigEvent>::try_new(event_ptr, &mut usp)?.read()?
        };
        notification_from_uapi(event)?
    };

    let prepared = task
        .get_thread_group()
        .prepare_posix_timer(clock, notification)?;
    let timer_id = prepared.id();
    {
        let usp_handle = task.clone_uspace_handle();
        let mut usp = usp_handle.lock();
        UserWritePtr::<i32>::try_new(timer_id_ptr, &mut usp)?.write(timer_id)?;
    }
    prepared.publish()?;
    Ok(0)
}

#[syscall(SYS_TIMER_GETTIME)]
fn sys_timer_gettime(
    timer_id: i32,
    #[validate_with(user_addr)] current_ptr: VirtAddr,
) -> Result<u64, SysError> {
    let task = get_current_task();
    let setting = task.get_thread_group().posix_timer_gettime(timer_id)?;
    let current = setting_to_uapi(setting);
    let usp_handle = task.clone_uspace_handle();
    let mut usp = usp_handle.lock();
    UserWritePtr::<ITimerSpec>::try_new(current_ptr, &mut usp)?.write(current)?;
    Ok(0)
}

#[syscall(SYS_TIMER_GETOVERRUN)]
fn sys_timer_getoverrun(timer_id: i32) -> Result<u64, SysError> {
    let overrun = get_current_task()
        .get_thread_group()
        .posix_timer_getoverrun(timer_id)?;
    Ok(overrun as u64)
}

#[syscall(SYS_TIMER_SETTIME)]
fn sys_timer_settime(
    timer_id: i32,
    flags: i32,
    #[validate_with(user_addr)] new_ptr: VirtAddr,
    old_ptr: u64,
) -> Result<u64, SysError> {
    let task = get_current_task();
    let new_setting = {
        let usp_handle = task.clone_uspace_handle();
        let mut usp = usp_handle.lock();
        let value = UserReadPtr::<ITimerSpec>::try_new(new_ptr, &mut usp)?.read()?;
        setting_from_uapi(value)?
    };

    let ignored_flags = flags & !TIMER_ABSTIME;
    if ignored_flags != 0 && !IGNORED_SETTIME_FLAGS_LOGGED.swap(true, Ordering::Relaxed) {
        // Linux's legacy timer_settime ABI tests only TIMER_ABSTIME and ignores
        // every other bit. Preserve that visible behavior, but keep it
        // observable so a future ABI revision cannot accidentally tighten it.
        knoticeln!(
            "timer_settime: ignoring legacy flag bits {:#x}",
            ignored_flags
        );
    }
    let old_setting = task.get_thread_group().posix_timer_settime(
        timer_id,
        new_setting,
        flags & TIMER_ABSTIME != 0,
    )?;

    // Linux applies the new setting before old-value copyout. An EFAULT here
    // is fail-forward and must not roll the timer back to its previous arm.
    if let Some(old_ptr) = (user_addr.nullable())(old_ptr)? {
        let usp_handle = task.clone_uspace_handle();
        let mut usp = usp_handle.lock();
        UserWritePtr::<ITimerSpec>::try_new(old_ptr, &mut usp)?
            .write(setting_to_uapi(old_setting))?;
    }
    Ok(0)
}

#[syscall(SYS_TIMER_DELETE)]
fn sys_timer_delete(timer_id: i32) -> Result<u64, SysError> {
    get_current_task()
        .get_thread_group()
        .delete_posix_timer(timer_id)?;
    Ok(0)
}

fn timer_clock(clock_id: i32) -> Result<PosixTimerClock, SysError> {
    match clock_id {
        CLOCK_REALTIME => Ok(PosixTimerClock::Realtime),
        CLOCK_MONOTONIC => Ok(PosixTimerClock::Monotonic),
        CLOCK_BOOTTIME => Ok(PosixTimerClock::Boottime),
        CLOCK_PROCESS_CPUTIME_ID | CLOCK_THREAD_CPUTIME_ID => {
            if !CPU_TIMER_UNSUPPORTED_LOGGED.swap(true, Ordering::Relaxed) {
                knoticeln!(
                    "timer_create: CPU-time clocks require scheduler-driven timers and are unsupported"
                );
            }
            Err(SysError::NotSupported)
        },
        _ => Err(SysError::InvalidArgument),
    }
}

fn notification_from_uapi(event: SigEvent) -> Result<PosixTimerNotification, SysError> {
    match event.sigev_notify {
        SIGEV_NONE => Ok(PosixTimerNotification::None),
        SIGEV_SIGNAL => Ok(PosixTimerNotification::Signal {
            no: SigNo::try_from_syscall_arg(event.sigev_signo as u64)?,
            sigval: event.sigev_value,
        }),
        unsupported => {
            if !NOTIFICATION_UNSUPPORTED_LOGGED.swap(true, Ordering::Relaxed) {
                knoticeln!(
                    "timer_create: sigev_notify={} is unsupported; only SIGEV_NONE and SIGEV_SIGNAL are available",
                    unsupported
                );
            }
            Err(SysError::NotSupported)
        },
    }
}

fn setting_from_uapi(value: ITimerSpec) -> Result<PosixTimerSetting, SysError> {
    Ok(PosixTimerSetting {
        value_ns: timespec_to_ns(value.it_value)?,
        interval_ns: timespec_to_ns(value.it_interval)?,
    })
}

fn setting_to_uapi(value: PosixTimerSetting) -> ITimerSpec {
    ITimerSpec {
        it_interval: ns_to_timespec(value.interval_ns),
        it_value: ns_to_timespec(value.value_ns),
    }
}

fn timespec_to_ns(value: TimeSpec) -> Result<u64, SysError> {
    if value.tv_sec < 0 || value.tv_nsec < 0 || value.tv_nsec >= NSEC_PER_SEC as i64 {
        return Err(SysError::InvalidArgument);
    }
    (value.tv_sec as u64)
        .checked_mul(NSEC_PER_SEC)
        .and_then(|seconds| seconds.checked_add(value.tv_nsec as u64))
        .ok_or(SysError::InvalidArgument)
}

fn ns_to_timespec(value: u64) -> TimeSpec {
    TimeSpec {
        tv_sec: (value / NSEC_PER_SEC) as i64,
        tv_nsec: (value % NSEC_PER_SEC) as i64,
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn native_timer_uapi_layout_and_timespec_validation_are_fixed() {
        assert_eq!(core::mem::size_of::<SigEvent>(), 64);
        assert_eq!(core::mem::size_of::<ITimerSpec>(), 32);
        assert_eq!(
            timespec_to_ns(TimeSpec {
                tv_sec: 1,
                tv_nsec: 2
            }),
            Ok(NSEC_PER_SEC + 2)
        );
        assert_eq!(
            timespec_to_ns(TimeSpec {
                tv_sec: -1,
                tv_nsec: 0
            }),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(
            timespec_to_ns(TimeSpec {
                tv_sec: 0,
                tv_nsec: NSEC_PER_SEC as i64
            }),
            Err(SysError::InvalidArgument)
        );
    }

    #[kunit]
    fn timer_clock_and_notification_matrices_are_explicit() {
        assert_eq!(timer_clock(CLOCK_REALTIME), Ok(PosixTimerClock::Realtime));
        assert_eq!(timer_clock(CLOCK_MONOTONIC), Ok(PosixTimerClock::Monotonic));
        assert_eq!(timer_clock(CLOCK_BOOTTIME), Ok(PosixTimerClock::Boottime));
        assert_eq!(timer_clock(4), Err(SysError::InvalidArgument));
        assert_eq!(
            notification_from_uapi(SigEvent {
                sigev_notify: SIGEV_NONE,
                sigev_signo: -1,
                ..SigEvent::default()
            }),
            Ok(PosixTimerNotification::None)
        );
    }
}
