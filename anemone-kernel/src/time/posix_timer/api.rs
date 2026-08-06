use anemone_abi::time::linux::{
    ITimerSpec, SigEvent, TimeSpec,
    clock::{
        CLOCK_BOOTTIME, CLOCK_MONOTONIC, CLOCK_PROCESS_CPUTIME_ID, CLOCK_REALTIME,
        CLOCK_THREAD_CPUTIME_ID, TIMER_ABSTIME,
    },
    posix_timer::{SIGEV_NONE, SIGEV_SIGNAL, SIGEV_THREAD, SIGEV_THREAD_ID},
};

use crate::{
    prelude::*,
    syscall::{
        handler::TryFromSyscallArg,
        user_access::{SyscallArgValidatorExt as _, UserReadPtr, UserWritePtr, user_addr},
    },
    task::{
        get_task_and_thread_group,
        sig::SigNo,
        task_posix_timer::{PosixTimerClock, PosixTimerNotification, PosixTimerSetting},
    },
};

const NSEC_PER_SEC: u64 = 1_000_000_000;

#[syscall(SYS_TIMER_CREATE)]
fn sys_timer_create(
    clock_id: i32,
    event_ptr: u64,
    #[validate_with(user_addr)] timer_id_ptr: VirtAddr,
) -> Result<u64, SysError> {
    let clock = timer_clock(clock_id)?;
    let task = get_current_task();
    let owner = task.get_thread_group();
    let notification = if event_ptr == 0 {
        PosixTimerNotification::DefaultSignal
    } else {
        let event = {
            let event_ptr = user_addr(event_ptr)?;
            let usp_handle = task.clone_uspace_handle();
            let mut usp = usp_handle.lock();
            UserReadPtr::<SigEvent>::try_new(event_ptr, &mut usp)?.read()?
        };
        notification_from_uapi(&owner, event)?
    };

    let prepared = owner.prepare_posix_timer(clock, notification)?;
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
    // Linux accepts every non-TIMER_ABSTIME bit, so record this compatibility
    // choice once without turning normal legacy use into a persistent warning.
    static IGNORED_SETTIME_FLAGS_LOGGED: AtomicBool = AtomicBool::new(false);

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
            knoticeln!(
                "timer_create: clock_id={} requires scheduler-driven CPU timers; errno=EOPNOTSUPP",
                clock_id
            );
            Err(SysError::NotSupported)
        },
        _ => Err(SysError::InvalidArgument),
    }
}

fn notification_from_uapi(
    owner: &Arc<ThreadGroup>,
    event: SigEvent,
) -> Result<PosixTimerNotification, SysError> {
    match event.sigev_notify {
        SIGEV_NONE => Ok(PosixTimerNotification::None),
        SIGEV_SIGNAL => Ok(PosixTimerNotification::Signal {
            no: SigNo::try_from_syscall_arg(event.sigev_signo as u64)?,
            sigval: event.sigev_value,
        }),
        SIGEV_THREAD_ID => {
            let no = SigNo::try_from_syscall_arg(event.sigev_signo as u64)?;
            let target_tid = event.sigev_notify_thread_id();
            if target_tid <= 0 {
                return Err(SysError::InvalidArgument);
            }
            let (target, target_owner) = get_task_and_thread_group(&Tid::new(target_tid as u32))
                .ok_or(SysError::InvalidArgument)?;
            if !Arc::ptr_eq(owner, &target_owner) {
                return Err(SysError::InvalidArgument);
            }
            Ok(PosixTimerNotification::ThreadSignal {
                target,
                no,
                sigval: event.sigev_value,
            })
        },
        SIGEV_THREAD => {
            knoticeln!(
                "timer_create: SIGEV_THREAD is intentionally unsupported; kernel-side userspace callback execution is outside this ABI; errno=EOPNOTSUPP"
            );
            Err(SysError::NotSupported)
        },
        _ => Err(SysError::InvalidArgument),
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
        assert_eq!(core::mem::align_of::<SigEvent>(), 8);
        assert_eq!(core::mem::offset_of!(SigEvent, sigev_value), 0);
        assert_eq!(core::mem::offset_of!(SigEvent, sigev_signo), 8);
        assert_eq!(core::mem::offset_of!(SigEvent, sigev_notify), 12);
        assert_eq!(core::mem::offset_of!(SigEvent, _sigev_un), 16);
        let mut thread_event = SigEvent::default();
        thread_event._sigev_un[0] = 0x1234_5678;
        assert_eq!(thread_event.sigev_notify_thread_id(), 0x1234_5678);
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
        let target = get_current_task();
        let owner = target.get_thread_group();
        assert_eq!(timer_clock(CLOCK_REALTIME), Ok(PosixTimerClock::Realtime));
        assert_eq!(timer_clock(CLOCK_MONOTONIC), Ok(PosixTimerClock::Monotonic));
        assert_eq!(timer_clock(CLOCK_BOOTTIME), Ok(PosixTimerClock::Boottime));
        assert_eq!(
            timer_clock(CLOCK_PROCESS_CPUTIME_ID),
            Err(SysError::NotSupported)
        );
        assert_eq!(timer_clock(4), Err(SysError::InvalidArgument));
        assert!(matches!(
            notification_from_uapi(
                &owner,
                SigEvent {
                    sigev_notify: SIGEV_NONE,
                    sigev_signo: -1,
                    ..SigEvent::default()
                },
            ),
            Ok(PosixTimerNotification::None)
        ));
        assert!(matches!(
            notification_from_uapi(
                &owner,
                SigEvent {
                    sigev_notify: SIGEV_THREAD,
                    ..SigEvent::default()
                },
            ),
            Err(SysError::NotSupported)
        ));

        let mut thread_event = SigEvent {
            sigev_notify: SIGEV_THREAD_ID,
            sigev_signo: SigNo::SIGUSR1.as_usize() as i32,
            ..SigEvent::default()
        };
        thread_event._sigev_un[0] = target.tid().get() as i32;
        let notification = notification_from_uapi(&owner, thread_event).unwrap();
        let PosixTimerNotification::ThreadSignal {
            target: decoded,
            no,
            ..
        } = notification
        else {
            panic!("SIGEV_THREAD_ID did not decode to a thread signal")
        };
        assert!(Arc::ptr_eq(&decoded, &target));
        assert_eq!(no, SigNo::SIGUSR1);

        thread_event._sigev_un[0] = 0;
        assert!(matches!(
            notification_from_uapi(&owner, thread_event),
            Err(SysError::InvalidArgument)
        ));
        thread_event.sigev_notify = SIGEV_THREAD_ID | 0x20;
        assert!(matches!(
            notification_from_uapi(&owner, thread_event),
            Err(SysError::InvalidArgument)
        ));
    }
}
