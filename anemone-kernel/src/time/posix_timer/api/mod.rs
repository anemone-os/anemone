mod timer_create;
mod timer_delete;
mod timer_getoverrun;
mod timer_gettime;
mod timer_settime;

use anemone_abi::time::linux::{
    ITimerSpec, SigEvent, TimeSpec,
    clock::{
        CLOCK_BOOTTIME, CLOCK_MONOTONIC, CLOCK_PROCESS_CPUTIME_ID, CLOCK_REALTIME,
        CLOCK_THREAD_CPUTIME_ID,
    },
    posix_timer::{SIGEV_NONE, SIGEV_SIGNAL, SIGEV_THREAD, SIGEV_THREAD_ID},
};

use crate::{
    prelude::*,
    syscall::handler::TryFromSyscallArg,
    task::{
        get_task_and_thread_group,
        sig::SigNo,
        task_posix_timer::{PosixTimerClock, PosixTimerNotification, PosixTimerSetting},
    },
};

const NSEC_PER_SEC: u64 = 1_000_000_000;

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
