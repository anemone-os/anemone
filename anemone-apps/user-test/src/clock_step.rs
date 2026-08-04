use core::{
    mem::size_of,
    sync::atomic::{AtomicUsize, Ordering},
};

use anemone_rs::{
    abi::{
        process::linux::signal::{SigAction, SigSet},
        syscall::{
            SYS_CLOCK_ADJTIME, SYS_CLOCK_GETTIME, SYS_CLOCK_NANOSLEEP, SYS_CLOCK_SETTIME,
            SYS_SETITIMER, SYS_SETUID, SYS_TIMERFD_CREATE, SYS_TIMERFD_GETTIME,
            SYS_TIMERFD_SETTIME, syscall,
        },
        time::linux::{
            ITimerSpec, TimeSpec, TimeVal, Timex,
            clock::{
                CLOCK_BOOTTIME, CLOCK_MONOTONIC, CLOCK_MONOTONIC_RAW, CLOCK_PROCESS_CPUTIME_ID,
                CLOCK_REALTIME, TIMER_ABSTIME,
            },
            itimer::{ITIMER_REAL, OldITimerVal},
            timerfd::{TFD_TIMER_ABSTIME, TFD_TIMER_CANCEL_ON_SET},
            timex::{ADJ_FREQUENCY, ADJ_NANO, ADJ_SETOFFSET, STA_UNSYNC, TIME_ERROR},
        },
    },
    os::linux::{
        fs::{PipeFlags, close, pipe2, read, write},
        process::{
            WStatus, WStatusRaw, WaitFor, WaitOptions, exit, fork,
            signal::{SigNo, sigaction},
            wait4,
        },
    },
    prelude::*,
};

const NSEC_PER_SEC: u64 = 1_000_000_000;
static SIGALRM_DELIVERIES: AtomicUsize = AtomicUsize::new(0);

extern "C" fn sigalrm_handler(_signo: i32) {
    SIGALRM_DELIVERIES.fetch_add(1, Ordering::Relaxed);
}

fn ns_to_timespec(ns: u64) -> TimeSpec {
    TimeSpec {
        tv_sec: (ns / NSEC_PER_SEC) as i64,
        tv_nsec: (ns % NSEC_PER_SEC) as i64,
    }
}

fn clock_ns(clock_id: i32) -> u64 {
    let mut value = TimeSpec::default();
    unsafe {
        syscall(
            SYS_CLOCK_GETTIME,
            clock_id as u64,
            (&mut value as *mut TimeSpec) as u64,
            0,
            0,
            0,
            0,
        )
        .unwrap();
    }
    (value.tv_sec as u64)
        .checked_mul(NSEC_PER_SEC)
        .and_then(|seconds| seconds.checked_add(value.tv_nsec as u64))
        .unwrap()
}

fn set_realtime(target_ns: u64) -> Result<(), Errno> {
    let target = ns_to_timespec(target_ns);
    unsafe {
        syscall(
            SYS_CLOCK_SETTIME,
            CLOCK_REALTIME as u64,
            (&target as *const TimeSpec) as u64,
            0,
            0,
            0,
            0,
        )?;
    }
    Ok(())
}

fn step_realtime(delta_ns: i64) {
    let now_ns = clock_ns(CLOCK_REALTIME);
    let target_ns = if delta_ns >= 0 {
        now_ns.checked_add(delta_ns as u64).unwrap()
    } else {
        now_ns.checked_sub(delta_ns.unsigned_abs()).unwrap()
    };
    set_realtime(target_ns).unwrap();
}

fn clock_nanosleep(
    clock_id: i32,
    flags: i32,
    request: &TimeSpec,
    remaining: Option<&mut TimeSpec>,
) -> Result<(), Errno> {
    let remaining = remaining
        .map(|value| value as *mut TimeSpec as u64)
        .unwrap_or(0);
    unsafe {
        syscall(
            SYS_CLOCK_NANOSLEEP,
            clock_id as u64,
            flags as u64,
            (request as *const TimeSpec) as u64,
            remaining,
            0,
            0,
        )?;
    }
    Ok(())
}

fn sleep_relative(clock_id: i32, ns: u64) {
    clock_nanosleep(clock_id, 0, &ns_to_timespec(ns), None).unwrap();
}

fn wait_child_ok(pid: u32, name: &str) {
    loop {
        let mut status = WStatusRaw::EMPTY;
        match wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut status),
            WaitOptions::empty(),
        ) {
            Ok(Some(waited)) => {
                assert_eq!(waited, pid, "{name}: waited for the wrong child");
                match status.read() {
                    WStatus::Exited(0) => return,
                    other => panic!("{name}: child failed: {other:?}"),
                }
            },
            Err(EINTR) => continue,
            other => panic!("{name}: wait4 failed: {other:?}"),
        }
    }
}

fn notify_parent(fd: u32) {
    assert_eq!(write(fd, &[1]).unwrap(), 1);
}

fn wait_child_ready(fd: u32) {
    let mut ready = [0_u8; 1];
    assert_eq!(read(fd, &mut ready).unwrap(), 1);
}

fn verify_set_and_adjust_abi() {
    let valid = ns_to_timespec(clock_ns(CLOCK_REALTIME).checked_add(NSEC_PER_SEC).unwrap());
    assert_eq!(
        unsafe {
            syscall(
                SYS_CLOCK_SETTIME,
                CLOCK_MONOTONIC as u64,
                (&valid as *const TimeSpec) as u64,
                0,
                0,
                0,
                0,
            )
        },
        Err(EINVAL)
    );
    let invalid = TimeSpec {
        tv_sec: 0,
        tv_nsec: NSEC_PER_SEC as i64,
    };
    assert_eq!(
        unsafe {
            syscall(
                SYS_CLOCK_SETTIME,
                CLOCK_REALTIME as u64,
                (&invalid as *const TimeSpec) as u64,
                0,
                0,
                0,
                0,
            )
        },
        Err(EINVAL)
    );
    assert_eq!(
        unsafe { syscall(SYS_CLOCK_SETTIME, CLOCK_REALTIME as u64, 1, 0, 0, 0, 0) },
        Err(EFAULT)
    );

    let mut query = Timex::default();
    assert_eq!(
        unsafe {
            syscall(
                SYS_CLOCK_ADJTIME,
                CLOCK_REALTIME as u64,
                (&mut query as *mut Timex) as u64,
                0,
                0,
                0,
                0,
            )
        },
        Ok(TIME_ERROR as u64)
    );
    assert_eq!(query.status, STA_UNSYNC);
    assert!(query.precision > 0);

    let mut unsupported = Timex {
        modes: ADJ_FREQUENCY,
        ..Timex::default()
    };
    assert_eq!(
        unsafe {
            syscall(
                SYS_CLOCK_ADJTIME,
                CLOCK_REALTIME as u64,
                (&mut unsupported as *mut Timex) as u64,
                0,
                0,
                0,
                0,
            )
        },
        Err(EOPNOTSUPP)
    );
    unsupported.modes = ADJ_NANO;
    assert_eq!(
        unsafe {
            syscall(
                SYS_CLOCK_ADJTIME,
                CLOCK_REALTIME as u64,
                (&mut unsupported as *mut Timex) as u64,
                0,
                0,
                0,
                0,
            )
        },
        Err(EINVAL)
    );
    assert_eq!(
        unsafe { syscall(SYS_CLOCK_ADJTIME, CLOCK_REALTIME as u64, 1, 0, 0, 0, 0) },
        Err(EFAULT)
    );

    let before = clock_ns(CLOCK_REALTIME);
    let mut adjust = Timex {
        modes: ADJ_SETOFFSET | ADJ_NANO,
        ..Timex::default()
    };
    adjust.time.tv_usec = 100_000_000;
    assert_eq!(
        unsafe {
            syscall(
                SYS_CLOCK_ADJTIME,
                CLOCK_REALTIME as u64,
                (&mut adjust as *mut Timex) as u64,
                0,
                0,
                0,
                0,
            )
        },
        Ok(TIME_ERROR as u64)
    );
    assert!(clock_ns(CLOCK_REALTIME) >= before + 100_000_000);
}

fn verify_sleep_matrix() {
    for clock_id in [CLOCK_REALTIME, CLOCK_MONOTONIC, CLOCK_BOOTTIME] {
        let before = clock_ns(CLOCK_MONOTONIC);
        sleep_relative(clock_id, 20_000_000);
        assert!(clock_ns(CLOCK_MONOTONIC) - before >= 20_000_000);
    }
    for clock_id in [CLOCK_REALTIME, CLOCK_MONOTONIC, CLOCK_BOOTTIME] {
        let deadline = ns_to_timespec(clock_ns(clock_id).checked_add(20_000_000).unwrap());
        clock_nanosleep(clock_id, TIMER_ABSTIME, &deadline, None).unwrap();
        assert!(clock_ns(clock_id) >= clock_ns_from_timespec(deadline));
    }
    assert_eq!(
        clock_nanosleep(
            CLOCK_PROCESS_CPUTIME_ID,
            0,
            &ns_to_timespec(1_000_000),
            None,
        ),
        Err(EOPNOTSUPP)
    );
    assert_eq!(
        clock_nanosleep(CLOCK_MONOTONIC_RAW, 0, &ns_to_timespec(1_000_000), None,),
        Err(EINVAL)
    );
    clock_nanosleep(CLOCK_MONOTONIC, 0x100, &ns_to_timespec(1_000_000), None).unwrap();
    assert_eq!(
        unsafe { syscall(SYS_CLOCK_NANOSLEEP, CLOCK_MONOTONIC as u64, 0, 1, 0, 0, 0,) },
        Err(EFAULT)
    );

    verify_relative_sleep_remaining();
}

fn verify_relative_sleep_remaining() {
    SIGALRM_DELIVERIES.store(0, Ordering::Relaxed);
    let action = SigAction {
        sighandler: sigalrm_handler as *const (),
        sa_flags: 0,
        sa_restorer: core::ptr::null(),
        sa_mask: SigSet { bits: 0 },
    };
    let mut old_action = SigAction {
        sighandler: core::ptr::null(),
        sa_flags: 0,
        sa_restorer: core::ptr::null(),
        sa_mask: SigSet { bits: 0 },
    };
    sigaction(SigNo::SIGALRM, Some(&action), Some(&mut old_action)).unwrap();
    let alarm = OldITimerVal {
        it_interval: TimeVal::default(),
        it_value: TimeVal {
            tv_sec: 0,
            tv_usec: 20_000,
        },
    };
    unsafe {
        syscall(
            SYS_SETITIMER,
            ITIMER_REAL as u64,
            (&alarm as *const OldITimerVal) as u64,
            0,
            0,
            0,
            0,
        )
        .unwrap();
    }

    let mut remaining = TimeSpec::default();
    assert_eq!(
        clock_nanosleep(
            CLOCK_MONOTONIC,
            0,
            &ns_to_timespec(NSEC_PER_SEC),
            Some(&mut remaining),
        ),
        Err(EINTR)
    );
    let remaining_ns = clock_ns_from_timespec(remaining);
    assert!((100_000_000..NSEC_PER_SEC).contains(&remaining_ns));
    assert_eq!(SIGALRM_DELIVERIES.load(Ordering::Relaxed), 1);
    sigaction(SigNo::SIGALRM, Some(&old_action), None).unwrap();
}

fn clock_ns_from_timespec(value: TimeSpec) -> u64 {
    (value.tv_sec as u64) * NSEC_PER_SEC + value.tv_nsec as u64
}

fn verify_absolute_realtime_step(direction: i64) {
    let (read_fd, write_fd) = pipe2(PipeFlags::empty()).unwrap();
    let deadline_ns = clock_ns(CLOCK_REALTIME).checked_add(500_000_000).unwrap();
    match fork().unwrap() {
        Some(pid) => {
            close(write_fd).unwrap();
            wait_child_ready(read_fd);
            sleep_relative(CLOCK_MONOTONIC, 20_000_000);
            step_realtime(direction);
            wait_child_ok(pid, "absolute realtime step");
            close(read_fd).unwrap();
        },
        None => {
            close(read_fd).unwrap();
            notify_parent(write_fd);
            let start = clock_ns(CLOCK_MONOTONIC);
            clock_nanosleep(
                CLOCK_REALTIME,
                TIMER_ABSTIME,
                &ns_to_timespec(deadline_ns),
                None,
            )
            .unwrap();
            let elapsed = clock_ns(CLOCK_MONOTONIC) - start;
            if direction > 0 {
                assert!(elapsed < 300_000_000);
            } else {
                assert!(elapsed >= 800_000_000);
            }
            close(write_fd).unwrap();
            exit(0);
        },
    }
}

fn timerfd_create(clock_id: i32) -> u32 {
    unsafe { syscall(SYS_TIMERFD_CREATE, clock_id as u64, 0, 0, 0, 0, 0).unwrap() as u32 }
}

fn timerfd_settime(fd: u32, flags: u32, value: ITimerSpec) -> Result<(), Errno> {
    unsafe {
        syscall(
            SYS_TIMERFD_SETTIME,
            fd as u64,
            flags as u64,
            (&value as *const ITimerSpec) as u64,
            0,
            0,
            0,
        )?;
    }
    Ok(())
}

fn timerfd_gettime(fd: u32) -> ITimerSpec {
    let mut value = ITimerSpec::default();
    unsafe {
        syscall(
            SYS_TIMERFD_GETTIME,
            fd as u64,
            (&mut value as *mut ITimerSpec) as u64,
            0,
            0,
            0,
            0,
        )
        .unwrap();
    }
    value
}

fn read_expirations(fd: u32) -> Result<u64, Errno> {
    let mut value = [0_u8; size_of::<u64>()];
    assert_eq!(read(fd, &mut value)?, value.len());
    Ok(u64::from_le_bytes(value))
}

fn verify_timerfd_realtime_steps() {
    let relative = timerfd_create(CLOCK_REALTIME);
    timerfd_settime(
        relative,
        0,
        ITimerSpec {
            it_interval: TimeSpec::default(),
            it_value: ns_to_timespec(500_000_000),
        },
    )
    .unwrap();
    step_realtime(2_000_000_000);
    assert!(clock_ns_from_timespec(timerfd_gettime(relative).it_value) > 100_000_000);
    assert_eq!(read_expirations(relative).unwrap(), 1);
    close(relative).unwrap();

    let absolute = timerfd_create(CLOCK_REALTIME);
    let target = clock_ns(CLOCK_REALTIME).checked_add(500_000_000).unwrap();
    timerfd_settime(
        absolute,
        TFD_TIMER_ABSTIME,
        ITimerSpec {
            it_interval: TimeSpec::default(),
            it_value: ns_to_timespec(target),
        },
    )
    .unwrap();
    step_realtime(-300_000_000);
    assert!(clock_ns_from_timespec(timerfd_gettime(absolute).it_value) >= 700_000_000);
    step_realtime(1_000_000_000);
    assert_eq!(read_expirations(absolute).unwrap(), 1);
    close(absolute).unwrap();

    let cancelled = timerfd_create(CLOCK_REALTIME);
    let target = clock_ns(CLOCK_REALTIME)
        .checked_add(5 * NSEC_PER_SEC)
        .unwrap();
    timerfd_settime(
        cancelled,
        TFD_TIMER_ABSTIME | TFD_TIMER_CANCEL_ON_SET,
        ITimerSpec {
            it_interval: TimeSpec::default(),
            it_value: ns_to_timespec(target),
        },
    )
    .unwrap();
    step_realtime(100_000_000);
    assert_eq!(read_expirations(cancelled), Err(ECANCELED));
    close(cancelled).unwrap();

    let monotonic = timerfd_create(CLOCK_MONOTONIC);
    assert_eq!(
        timerfd_settime(
            monotonic,
            TFD_TIMER_ABSTIME | TFD_TIMER_CANCEL_ON_SET,
            ITimerSpec {
                it_interval: TimeSpec::default(),
                it_value: ns_to_timespec(clock_ns(CLOCK_MONOTONIC) + NSEC_PER_SEC),
            },
        ),
        Err(EINVAL)
    );
    close(monotonic).unwrap();
}

fn verify_settime_permission() {
    match fork().unwrap() {
        Some(pid) => wait_child_ok(pid, "clock_settime permission"),
        None => {
            unsafe { syscall(SYS_SETUID, 1000, 0, 0, 0, 0, 0).unwrap() };
            let target = ns_to_timespec(clock_ns(CLOCK_REALTIME) + NSEC_PER_SEC);
            assert_eq!(
                unsafe {
                    syscall(
                        SYS_CLOCK_SETTIME,
                        CLOCK_REALTIME as u64,
                        (&target as *const TimeSpec) as u64,
                        0,
                        0,
                        0,
                        0,
                    )
                },
                Err(EPERM)
            );
            exit(0);
        },
    }
}

pub(crate) fn verify_clock_steps() {
    verify_set_and_adjust_abi();
    set_realtime(clock_ns(CLOCK_REALTIME) + 5 * NSEC_PER_SEC).unwrap();
    verify_sleep_matrix();
    verify_absolute_realtime_step(1_000_000_000);
    verify_absolute_realtime_step(-500_000_000);
    verify_timerfd_realtime_steps();
    verify_settime_permission();

    set_realtime(clock_ns(CLOCK_MONOTONIC) + NSEC_PER_SEC).unwrap();
    println!("clock-step: realtime mutation, sleep, and timerfd checks passed");
}
