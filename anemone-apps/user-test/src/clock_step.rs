//! Userspace oracle for realtime mutation and step-sensitive waits.
//!
//! The cases deliberately mix direct clock syscalls, forked sleepers, timerfd,
//! and ITIMER_REAL so the test observes both ABI results and cross-owner wakeup
//! behavior rather than only checking the timekeeper value.

use core::{
    mem::size_of,
    sync::atomic::{AtomicUsize, Ordering},
};

use anemone_rs::{
    abi::{
        process::linux::{
            futex::{
                FUTEX_BITSET_MATCH_ANY, FUTEX_CLOCK_REALTIME, FUTEX_PRIVATE_FLAG, FUTEX_WAIT,
                FUTEX_WAIT_BITSET,
            },
            signal::{SigAction, SigSet},
        },
        syscall::{
            SYS_CLOCK_ADJTIME, SYS_CLOCK_GETTIME, SYS_CLOCK_NANOSLEEP, SYS_CLOCK_SETTIME,
            SYS_FUTEX, SYS_SETITIMER, SYS_SETUID, SYS_TIMERFD_CREATE, SYS_TIMERFD_GETTIME,
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
    // Keep input-shape, pointer-fault, supported-operation, and returned clock
    // state checks together: they define the externally visible Gate 3 subset.
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
    // Relative and absolute forms share clock admission but differ in remaining
    // time and realtime-step behavior. Unsupported CPU clocks must remain
    // distinguishable from invalid clock IDs.
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
    // ITIMER_REAL supplies a real asynchronous signal so EINTR and `rmtp` are
    // validated through the production signal/wait path.
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
    // The pipe prevents the parent from stepping before the child reaches the
    // sleep branch. A short monotonic pause then gives the child a registration
    // window without making the tested elapsed bound depend on that pause.
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

fn verify_futex_realtime_step(direction: i64) {
    // WAIT_BITSET keeps the userspace absolute deadline. The pipe and short
    // monotonic pause ensure the parent step races with an installed waiter,
    // not with child startup.
    let (read_fd, write_fd) = pipe2(PipeFlags::empty()).unwrap();
    let deadline_ns = clock_ns(CLOCK_REALTIME).checked_add(500_000_000).unwrap();
    match fork().unwrap() {
        Some(pid) => {
            close(write_fd).unwrap();
            wait_child_ready(read_fd);
            sleep_relative(CLOCK_MONOTONIC, 20_000_000);
            step_realtime(direction);
            wait_child_ok(pid, "futex realtime step");
            close(read_fd).unwrap();
        },
        None => {
            close(read_fd).unwrap();
            let word = 0_u32;
            let deadline = ns_to_timespec(deadline_ns);
            notify_parent(write_fd);
            let start = clock_ns(CLOCK_MONOTONIC);
            assert_eq!(
                unsafe {
                    syscall(
                        SYS_FUTEX,
                        (&word as *const u32) as u64,
                        (FUTEX_WAIT_BITSET | FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME) as u64,
                        0,
                        (&deadline as *const TimeSpec) as u64,
                        0,
                        FUTEX_BITSET_MATCH_ANY as u64,
                    )
                },
                Err(ETIMEDOUT)
            );
            let elapsed = clock_ns(CLOCK_MONOTONIC) - start;
            if direction > 0 {
                assert!(elapsed < 300_000_000);
            } else {
                assert!(elapsed >= 700_000_000);
            }
            close(write_fd).unwrap();
            exit(0);
        },
    }
}

fn verify_futex_realtime_abi() {
    let word = 0_u32;
    let timeout = ns_to_timespec(1);
    assert_eq!(
        unsafe {
            syscall(
                SYS_FUTEX,
                (&word as *const u32) as u64,
                (FUTEX_WAIT | FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME) as u64,
                0,
                (&timeout as *const TimeSpec) as u64,
                0,
                0,
            )
        },
        Err(ENOSYS)
    );
    verify_futex_realtime_step(1_000_000_000);
    verify_futex_realtime_step(-300_000_000);
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
    // Relative CLOCK_REALTIME is frozen to monotonic and must ignore a step.
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

    // Absolute CLOCK_REALTIME follows the mutable calendar in both directions.
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

    // CANCEL_ON_SET consumes the current arm as one ECANCELED read instead of
    // reporting an expiration on either the old or replacement timeline.
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
    // Isolate credential loss in a child; the main test process must retain
    // SYS_TIME to restore realtime for later user-test modules.
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
    verify_futex_realtime_abi();
    verify_timerfd_realtime_steps();
    verify_settime_permission();

    // Leave later tests near the boot-relative calendar baseline instead of
    // leaking this module's accumulated forward/backward steps.
    set_realtime(clock_ns(CLOCK_MONOTONIC) + NSEC_PER_SEC).unwrap();
    println!("clock-step: realtime mutation, sleep, futex, and timerfd checks passed");
}
