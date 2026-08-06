//! Userspace oracle for native POSIX timer objects and syscalls.

use core::{
    ptr::null_mut,
    sync::atomic::{AtomicI32, AtomicU64, AtomicUsize, Ordering},
};

use anemone_rs::{
    abi::{
        process::linux::{
            signal::{
                self as linux_signal, SA_SIGINFO, SigAction, SigInfo, SigInfoWrapper, SigSet,
            },
            ucontext::UContext,
        },
        syscall::{
            SYS_CLOCK_GETTIME, SYS_CLOCK_SETTIME, SYS_NANOSLEEP, SYS_RT_SIGTIMEDWAIT,
            SYS_TIMER_CREATE, SYS_TIMER_DELETE, SYS_TIMER_GETOVERRUN, SYS_TIMER_GETTIME,
            SYS_TIMER_SETTIME, syscall,
        },
        time::linux::{
            ITimerSpec, SigEvent, TimeSpec,
            clock::{
                CLOCK_BOOTTIME, CLOCK_MONOTONIC, CLOCK_PROCESS_CPUTIME_ID, CLOCK_REALTIME,
                TIMER_ABSTIME,
            },
            posix_timer::{SIGEV_NONE, SIGEV_SIGNAL, SIGEV_THREAD, SIGEV_THREAD_ID},
        },
    },
    os::linux::process::{
        CloneFlags, MmapFlags, MmapProt, WStatus, WStatusRaw, WaitFor, WaitOptions, exit, fork,
        gettid, mmap, sched_yield,
        signal::{self, SigNo, SigProcMaskHow, kill},
        spawn_raw_thread, wait4,
    },
    prelude::*,
};

const NSEC_PER_SEC: u64 = 1_000_000_000;
const RAW_THREAD_STACK_SIZE: usize = 64 * 1024;
static DELIVERIES: AtomicUsize = AtomicUsize::new(0);
static LAST_SIGNO: AtomicI32 = AtomicI32::new(0);
static LAST_TIMER_ID: AtomicI32 = AtomicI32::new(-1);
static LAST_OVERRUN: AtomicI32 = AtomicI32::new(-1);
static LAST_SIGVAL: AtomicU64 = AtomicU64::new(0);
static SEEN_SIGVALS: AtomicU64 = AtomicU64::new(0);

#[anemone_rs::signal_handler(siginfo)]
fn timer_handler(signo: SigNo, siginfo: *const SigInfo, ucontext: *const UContext) {
    assert!(!siginfo.is_null());
    assert!(!ucontext.is_null());
    let info = unsafe { &*siginfo };
    let timer = unsafe { info.fields.timer };
    LAST_SIGNO.store(signo.as_usize() as i32, Ordering::SeqCst);
    LAST_TIMER_ID.store(timer.tid, Ordering::SeqCst);
    LAST_OVERRUN.store(timer.overrun, Ordering::SeqCst);
    let sigval = timer.sigval.as_u64();
    LAST_SIGVAL.store(sigval, Ordering::SeqCst);
    if sigval < 64 {
        SEEN_SIGVALS.fetch_or(1 << sigval, Ordering::SeqCst);
    }
    DELIVERIES.fetch_add(1, Ordering::SeqCst);
}

fn empty_action() -> SigAction {
    SigAction {
        sighandler: core::ptr::null(),
        sa_flags: 0,
        sa_restorer: core::ptr::null(),
        sa_mask: SigSet { bits: 0 },
    }
}

fn install_handler(no: SigNo) -> SigAction {
    let action = SigAction {
        sighandler: timer_handler as *const (),
        sa_flags: SA_SIGINFO,
        sa_restorer: core::ptr::null(),
        sa_mask: SigSet { bits: 0 },
    };
    let mut old = empty_action();
    signal::sigaction(no, Some(&action), Some(&mut old)).unwrap();
    old
}

fn ns_to_timespec(ns: u64) -> TimeSpec {
    TimeSpec {
        tv_sec: (ns / NSEC_PER_SEC) as i64,
        tv_nsec: (ns % NSEC_PER_SEC) as i64,
    }
}

fn timespec_to_ns(value: TimeSpec) -> u64 {
    (value.tv_sec as u64)
        .checked_mul(NSEC_PER_SEC)
        .and_then(|seconds| seconds.checked_add(value.tv_nsec as u64))
        .unwrap()
}

fn sleep_ns(ns: u64) {
    let request = ns_to_timespec(ns);
    match unsafe {
        syscall(
            SYS_NANOSLEEP,
            (&request as *const TimeSpec) as u64,
            0,
            0,
            0,
            0,
            0,
        )
    } {
        Ok(_) | Err(EINTR) => {},
        other => panic!("POSIX timer oracle nanosleep failed: {other:?}"),
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

fn set_realtime(target_ns: u64) {
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
        )
        .unwrap();
    }
}

fn wait_for_deliveries(expected: usize) {
    for _ in 0..100 {
        if DELIVERIES.load(Ordering::SeqCst) >= expected {
            return;
        }
        sleep_ns(10_000_000);
    }
    panic!("POSIX timer delivery did not arrive");
}

fn create_timer(clock: i32, event: Option<&SigEvent>) -> Result<i32, Errno> {
    let mut id = -1_i32;
    unsafe {
        syscall(
            SYS_TIMER_CREATE,
            clock as u64,
            event
                .map(|event| event as *const SigEvent as u64)
                .unwrap_or(0),
            (&mut id as *mut i32) as u64,
            0,
            0,
            0,
        )?;
    }
    Ok(id)
}

fn set_timer(
    id: i32,
    flags: i32,
    value: ITimerSpec,
    old: Option<&mut ITimerSpec>,
) -> Result<(), Errno> {
    unsafe {
        syscall(
            SYS_TIMER_SETTIME,
            id as u64,
            flags as u64,
            (&value as *const ITimerSpec) as u64,
            old.map(|old| old as *mut ITimerSpec as u64).unwrap_or(0),
            0,
            0,
        )?;
    }
    Ok(())
}

fn get_timer(id: i32) -> Result<ITimerSpec, Errno> {
    let mut value = ITimerSpec::default();
    unsafe {
        syscall(
            SYS_TIMER_GETTIME,
            id as u64,
            (&mut value as *mut ITimerSpec) as u64,
            0,
            0,
            0,
            0,
        )?;
    }
    Ok(value)
}

fn get_overrun(id: i32) -> Result<i32, Errno> {
    unsafe { syscall(SYS_TIMER_GETOVERRUN, id as u64, 0, 0, 0, 0, 0).map(|value| value as i32) }
}

fn delete_timer(id: i32) -> Result<(), Errno> {
    unsafe {
        syscall(SYS_TIMER_DELETE, id as u64, 0, 0, 0, 0, 0)?;
    }
    Ok(())
}

fn signal_event(no: SigNo, sigval: u64) -> SigEvent {
    SigEvent {
        sigev_value: sigval,
        sigev_signo: no.as_usize() as i32,
        sigev_notify: SIGEV_SIGNAL,
        _sigev_un: [0; 12],
    }
}

fn thread_id_event(no: SigNo, sigval: u64, tid: i32) -> SigEvent {
    let mut event = SigEvent {
        sigev_value: sigval,
        sigev_signo: no.as_usize() as i32,
        sigev_notify: SIGEV_THREAD_ID,
        _sigev_un: [0; 12],
    };
    event._sigev_un[0] = tid;
    assert_eq!(event.sigev_notify_thread_id(), tid);
    event
}

fn raw_sigtimedwait(no: SigNo, timeout_ns: u64) -> Result<SigInfo, Errno> {
    let set = signal_set(no);
    let timeout = ns_to_timespec(timeout_ns);
    let mut info = SigInfoWrapper::default();
    unsafe {
        syscall(
            SYS_RT_SIGTIMEDWAIT,
            (&set as *const SigSet) as u64,
            (&mut info as *mut SigInfoWrapper) as u64,
            (&timeout as *const TimeSpec) as u64,
            core::mem::size_of::<SigSet>() as u64,
            0,
            0,
        )?;
        Ok(info.info)
    }
}

fn raw_thread_flags() -> CloneFlags {
    CloneFlags::VM
        | CloneFlags::FS
        | CloneFlags::FILES
        | CloneFlags::SIGHAND
        | CloneFlags::THREAD
        | CloneFlags::SYSVSEM
}

fn spawn_timer_thread(entry: extern "C" fn(usize) -> !, arg: usize) -> u32 {
    let stack = mmap(
        0,
        RAW_THREAD_STACK_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )
    .unwrap();
    let stack_top = unsafe { stack.as_ptr().add(RAW_THREAD_STACK_SIZE) };

    // Raw threads have no pthread-style join owner. These focused test stacks
    // intentionally remain mapped until user-test exits after the oracle run.
    unsafe {
        spawn_raw_thread(
            raw_thread_flags(),
            stack_top,
            None,
            null_mut(),
            None,
            entry,
            arg,
        )
        .unwrap()
    }
}

fn wait_for_bits(value: &AtomicUsize, expected: usize, what: &str) {
    for _ in 0..2_000 {
        if value.load(Ordering::SeqCst) & expected == expected {
            return;
        }
        sched_yield().unwrap();
        sleep_ns(500_000);
    }
    panic!("POSIX timer timed out waiting for {what}");
}

struct ExactWaitCase {
    ready: AtomicUsize,
    go: AtomicUsize,
    done: AtomicUsize,
    target_result: AtomicI32,
    decoy_result: AtomicI32,
    timer_id: AtomicI32,
    code: AtomicI32,
    sigval: AtomicU64,
    overrun: AtomicI32,
}

impl ExactWaitCase {
    fn new() -> Self {
        Self {
            ready: AtomicUsize::new(0),
            go: AtomicUsize::new(0),
            done: AtomicUsize::new(0),
            target_result: AtomicI32::new(0),
            decoy_result: AtomicI32::new(0),
            timer_id: AtomicI32::new(-1),
            code: AtomicI32::new(0),
            sigval: AtomicU64::new(0),
            overrun: AtomicI32::new(-1),
        }
    }
}

fn wait_for_thread_go(case: &ExactWaitCase, ready_bit: usize) {
    case.ready.fetch_or(ready_bit, Ordering::SeqCst);
    while case.go.load(Ordering::SeqCst) & ready_bit == 0 {
        sched_yield().unwrap();
    }
}

extern "C" fn exact_target_waiter(arg: usize) -> ! {
    let case = unsafe { &*(arg as *const ExactWaitCase) };
    let blocked = signal_set(SigNo::SIGUSR1);
    signal::sigprocmask(SigProcMaskHow::Block, Some(&blocked), None).unwrap();
    wait_for_thread_go(case, 1);
    match raw_sigtimedwait(SigNo::SIGUSR1, 500_000_000) {
        Ok(info) => {
            let timer = unsafe { info.fields.timer };
            case.target_result.store(info.si_signo, Ordering::SeqCst);
            case.timer_id.store(timer.tid, Ordering::SeqCst);
            case.code.store(info.si_code, Ordering::SeqCst);
            case.sigval.store(timer.sigval.as_u64(), Ordering::SeqCst);
            case.overrun.store(timer.overrun, Ordering::SeqCst);
        },
        Err(error) => case.target_result.store(-error, Ordering::SeqCst),
    }
    case.done.fetch_or(1, Ordering::SeqCst);
    exit(0)
}

extern "C" fn exact_decoy_waiter(arg: usize) -> ! {
    let case = unsafe { &*(arg as *const ExactWaitCase) };
    let blocked = signal_set(SigNo::SIGUSR1);
    signal::sigprocmask(SigProcMaskHow::Block, Some(&blocked), None).unwrap();
    wait_for_thread_go(case, 2);
    let result = match raw_sigtimedwait(SigNo::SIGUSR1, 250_000_000) {
        Ok(info) => info.si_signo,
        Err(error) => -error,
    };
    case.decoy_result.store(result, Ordering::SeqCst);
    case.done.fetch_or(2, Ordering::SeqCst);
    exit(0)
}

struct ExitWaitCase {
    ready: AtomicUsize,
    release: AtomicUsize,
    done: AtomicUsize,
}

impl ExitWaitCase {
    fn new() -> Self {
        Self {
            ready: AtomicUsize::new(0),
            release: AtomicUsize::new(0),
            done: AtomicUsize::new(0),
        }
    }
}

extern "C" fn exit_waiter(arg: usize) -> ! {
    let case = unsafe { &*(arg as *const ExitWaitCase) };
    case.ready.store(1, Ordering::SeqCst);
    while case.release.load(Ordering::SeqCst) == 0 {
        sched_yield().unwrap();
    }
    case.done.store(1, Ordering::SeqCst);
    exit(0)
}

fn one_shot(ns: u64) -> ITimerSpec {
    ITimerSpec {
        it_interval: TimeSpec::default(),
        it_value: ns_to_timespec(ns),
    }
}

fn verify_abi_and_fail_forward() {
    assert_eq!(
        unsafe { syscall(SYS_TIMER_CREATE, CLOCK_MONOTONIC as u64, 0, 1, 0, 0, 0) },
        Err(EFAULT)
    );
    assert_eq!(
        unsafe { syscall(SYS_TIMER_CREATE, CLOCK_MONOTONIC as u64, 1, 1, 0, 0, 0) },
        Err(EFAULT)
    );
    assert_eq!(
        create_timer(CLOCK_PROCESS_CPUTIME_ID, None),
        Err(EOPNOTSUPP)
    );
    assert_eq!(create_timer(4, None), Err(EINVAL));

    let unsupported = SigEvent {
        sigev_notify: SIGEV_THREAD,
        ..SigEvent::default()
    };
    assert_eq!(
        create_timer(CLOCK_MONOTONIC, Some(&unsupported)),
        Err(EOPNOTSUPP)
    );
    let bad_signal = SigEvent {
        sigev_notify: SIGEV_SIGNAL,
        sigev_signo: 0,
        ..SigEvent::default()
    };
    assert_eq!(
        create_timer(CLOCK_MONOTONIC, Some(&bad_signal)),
        Err(EINVAL)
    );

    let none = SigEvent {
        sigev_notify: SIGEV_NONE,
        ..SigEvent::default()
    };
    let id = create_timer(CLOCK_MONOTONIC, Some(&none)).unwrap();
    // Failed create copyout released ID 0 instead of publishing or leaking it.
    assert_eq!(id, 0);
    assert_eq!(
        unsafe { syscall(SYS_TIMER_GETTIME, id as u64, 1, 0, 0, 0, 0) },
        Err(EFAULT)
    );
    assert_eq!(
        unsafe { syscall(SYS_TIMER_SETTIME, id as u64, 0, 1, 0, 0, 0) },
        Err(EFAULT)
    );

    let invalid = ITimerSpec {
        it_interval: TimeSpec::default(),
        it_value: TimeSpec {
            tv_sec: 0,
            tv_nsec: NSEC_PER_SEC as i64,
        },
    };
    assert_eq!(set_timer(id, 0, invalid, None), Err(EINVAL));

    set_timer(id, 0, one_shot(NSEC_PER_SEC), None).unwrap();
    assert_eq!(
        unsafe {
            let replacement = one_shot(200_000_000);
            syscall(
                SYS_TIMER_SETTIME,
                id as u64,
                0,
                (&replacement as *const ITimerSpec) as u64,
                1,
                0,
                0,
            )
        },
        Err(EFAULT)
    );
    let current = get_timer(id).unwrap();
    assert!(current.it_value.tv_sec == 0 && current.it_value.tv_nsec > 0);
    assert!(current.it_value.tv_nsec <= 200_000_000);

    // Unknown bits are Linux legacy compatibility, not invalid input.
    set_timer(id, 0x4000, one_shot(20_000_000), None).unwrap();
    assert_eq!(get_overrun(id), Ok(0));
    delete_timer(id).unwrap();

    for invalid_id in [id, 0x1234_5678] {
        assert_eq!(get_timer(invalid_id), Err(EINVAL));
        assert_eq!(get_overrun(invalid_id), Err(EINVAL));
        assert_eq!(
            set_timer(invalid_id, 0, ITimerSpec::default(), None),
            Err(EINVAL)
        );
        assert_eq!(delete_timer(invalid_id), Err(EINVAL));
    }
    assert_eq!(unsafe { syscall(403, 0, 0, 0, 0, 0, 0) }, Err(ENOSYS));
}

fn verify_default_and_none() {
    DELIVERIES.store(0, Ordering::SeqCst);
    let id = create_timer(CLOCK_REALTIME, None).unwrap();
    set_timer(id, 0, one_shot(30_000_000), None).unwrap();
    wait_for_deliveries(1);
    assert_eq!(
        LAST_SIGNO.load(Ordering::SeqCst),
        SigNo::SIGALRM.as_usize() as i32
    );
    assert_eq!(LAST_TIMER_ID.load(Ordering::SeqCst), id);
    assert_eq!(LAST_SIGVAL.load(Ordering::SeqCst), id as u64);
    assert_eq!(get_timer(id).unwrap().it_value, TimeSpec::default());
    delete_timer(id).unwrap();

    let none = SigEvent {
        sigev_notify: SIGEV_NONE,
        ..SigEvent::default()
    };
    let id = create_timer(CLOCK_BOOTTIME, Some(&none)).unwrap();
    set_timer(id, TIMER_ABSTIME, one_shot(1), None).unwrap();
    sleep_ns(30_000_000);
    assert_eq!(get_timer(id).unwrap().it_value, TimeSpec::default());
    delete_timer(id).unwrap();
}

fn verify_realtime_timeline_selection() {
    let event = signal_event(SigNo::SIGUSR1, 11);
    let initial_offset = clock_ns(CLOCK_REALTIME).saturating_sub(clock_ns(CLOCK_MONOTONIC));

    // A relative realtime arm represents elapsed duration. A calendar jump
    // must not make it expire, so its authoritative target lives on monotonic.
    DELIVERIES.store(0, Ordering::SeqCst);
    let relative = create_timer(CLOCK_REALTIME, Some(&event)).unwrap();
    set_timer(relative, 0, one_shot(500_000_000), None).unwrap();
    set_realtime(clock_ns(CLOCK_REALTIME) + 2 * NSEC_PER_SEC);
    assert!(timespec_to_ns(get_timer(relative).unwrap().it_value) > 100_000_000);
    assert_eq!(DELIVERIES.load(Ordering::SeqCst), 0);
    sleep_ns(550_000_000);
    wait_for_deliveries(1);
    delete_timer(relative).unwrap();

    // An absolute realtime arm remains a calendar deadline. Moving realtime
    // backward extends its remaining value; crossing it forward expires it.
    DELIVERIES.store(0, Ordering::SeqCst);
    let absolute = create_timer(CLOCK_REALTIME, Some(&event)).unwrap();
    let target = clock_ns(CLOCK_REALTIME) + 300_000_000;
    set_timer(absolute, TIMER_ABSTIME, one_shot(target), None).unwrap();
    // Keep enough margin for a heavily loaded TCG guest; this still exercises
    // the same backward absolute-realtime deadline semantics.
    set_realtime(clock_ns(CLOCK_REALTIME) - 1_000_000_000);
    sleep_ns(350_000_000);
    assert!(timespec_to_ns(get_timer(absolute).unwrap().it_value) > 50_000_000);
    assert_eq!(DELIVERIES.load(Ordering::SeqCst), 0);
    set_realtime(target + 100_000_000);
    wait_for_deliveries(1);
    delete_timer(absolute).unwrap();

    // Do not leak calendar mutations into later local tests.
    set_realtime(clock_ns(CLOCK_MONOTONIC) + initial_offset);
}

fn signal_set(no: SigNo) -> SigSet {
    SigSet {
        bits: 1 << (no.as_usize() - 1),
    }
}

fn verify_thread_id_validation() {
    let tid = i32::try_from(gettid().unwrap()).unwrap();
    for invalid_tid in [0, -1, i32::MAX] {
        assert_eq!(
            create_timer(
                CLOCK_MONOTONIC,
                Some(&thread_id_event(SigNo::SIGUSR1, 0, invalid_tid)),
            ),
            Err(EINVAL)
        );
    }

    let mut mixed = thread_id_event(SigNo::SIGUSR1, 0, tid);
    mixed.sigev_notify |= 0x20;
    assert_eq!(create_timer(CLOCK_MONOTONIC, Some(&mixed)), Err(EINVAL));
    let unknown = SigEvent {
        sigev_notify: 99,
        ..SigEvent::default()
    };
    assert_eq!(create_timer(CLOCK_MONOTONIC, Some(&unknown)), Err(EINVAL));

    match fork().unwrap() {
        Some(pid) => {
            assert_eq!(
                create_timer(
                    CLOCK_MONOTONIC,
                    Some(&thread_id_event(
                        SigNo::SIGUSR1,
                        0,
                        i32::try_from(pid).unwrap(),
                    )),
                ),
                Err(EINVAL)
            );
            kill(pid as i32, SigNo::SIGKILL).unwrap();
            assert!(matches!(
                wait_status(pid, WaitOptions::empty()),
                WStatus::Signal(value) if value == SigNo::SIGKILL.as_usize() as i8
            ));
        },
        None => loop {
            sleep_ns(NSEC_PER_SEC);
        },
    }
}

fn verify_thread_id_exact_wait_and_dequeued_exit() {
    let usr1 = signal_set(SigNo::SIGUSR1);
    let mut old_mask = SigSet { bits: 0 };
    signal::sigprocmask(SigProcMaskHow::Block, Some(&usr1), Some(&mut old_mask)).unwrap();
    DELIVERIES.store(0, Ordering::SeqCst);

    let case = Box::leak(Box::new(ExactWaitCase::new()));
    let target_tid = spawn_timer_thread(exact_target_waiter, case as *const _ as usize);
    let decoy_tid = spawn_timer_thread(exact_decoy_waiter, case as *const _ as usize);
    assert_ne!(target_tid, decoy_tid);
    wait_for_bits(&case.ready, 3, "target and decoy readiness");

    let sigval = 0x5449_4401;
    let timer = create_timer(
        CLOCK_MONOTONIC,
        Some(&thread_id_event(
            SigNo::SIGUSR1,
            sigval,
            i32::try_from(target_tid).unwrap(),
        )),
    )
    .unwrap();
    // Release only the decoy for the expiry window. Both tasks keep SIGUSR1
    // blocked, so a shared fallback can only be observed by the decoy.
    case.go.store(2, Ordering::SeqCst);
    set_timer(
        timer,
        0,
        ITimerSpec {
            it_interval: ns_to_timespec(30_000_000),
            it_value: ns_to_timespec(30_000_000),
        },
        None,
    )
    .unwrap();
    sleep_ns(80_000_000);
    case.go.fetch_or(1, Ordering::SeqCst);
    wait_for_bits(&case.done, 3, "target and decoy completion");

    assert_eq!(
        case.target_result.load(Ordering::SeqCst),
        SigNo::SIGUSR1.as_usize() as i32
    );
    assert_eq!(case.decoy_result.load(Ordering::SeqCst), -EAGAIN);
    assert_eq!(case.code.load(Ordering::SeqCst), linux_signal::SI_TIMER);
    assert_eq!(case.timer_id.load(Ordering::SeqCst), timer);
    assert_eq!(case.sigval.load(Ordering::SeqCst), sigval);
    let delivered_overrun = case.overrun.load(Ordering::SeqCst);
    assert!(delivered_overrun > 0);
    assert_eq!(DELIVERIES.load(Ordering::SeqCst), 0);

    // The target exits immediately after synchronous dequeue, which has already
    // rearmed the periodic timer. Its next exact expiry must fail without TID
    // lookup or retargeting, while gettime keeps Linux's future projection.
    sleep_ns(100_000_000);
    let projection = get_timer(timer).unwrap();
    assert_eq!(timespec_to_ns(projection.it_interval), 30_000_000);
    assert!(timespec_to_ns(projection.it_value) <= 30_000_000);
    assert_eq!(get_overrun(timer), Ok(delivered_overrun));
    delete_timer(timer).unwrap();
    signal::sigprocmask(SigProcMaskHow::SetMask, Some(&old_mask), None).unwrap();
}

fn spawn_exit_target() -> (&'static ExitWaitCase, u32) {
    let case = Box::leak(Box::new(ExitWaitCase::new()));
    let tid = spawn_timer_thread(exit_waiter, case as *const _ as usize);
    wait_for_bits(&case.ready, 1, "exit target readiness");
    (case, tid)
}

fn release_exit_target(case: &ExitWaitCase) {
    case.release.store(1, Ordering::SeqCst);
    wait_for_bits(&case.done, 1, "target exit");
}

fn wait_for_closed_thread_id(tid: u32) {
    let event = thread_id_event(SigNo::SIGUSR1, 0x5449_445f, i32::try_from(tid).unwrap());
    for _ in 0..200 {
        match create_timer(CLOCK_MONOTONIC, Some(&event)) {
            Err(EINVAL) => return,
            Ok(timer) => delete_timer(timer).unwrap(),
            Err(error) => panic!("closed SIGEV_THREAD_ID target returned {error:?}"),
        }
        sleep_ns(1_000_000);
    }
    panic!("SIGEV_THREAD_ID target admission did not close");
}

fn assert_periodic_projection(timer: i32, interval_ns: u64) {
    let projection = get_timer(timer).unwrap();
    assert_eq!(timespec_to_ns(projection.it_interval), interval_ns);
    let remaining = timespec_to_ns(projection.it_value);
    assert!(remaining > 0 && remaining <= interval_ns);
}

fn verify_thread_id_exit_before_expiry_and_pending_flush() {
    let usr1 = signal_set(SigNo::SIGUSR1);
    let mut old_mask = SigSet { bits: 0 };
    signal::sigprocmask(SigProcMaskHow::Block, Some(&usr1), Some(&mut old_mask)).unwrap();

    let (before_expiry, tid) = spawn_exit_target();
    let timer = create_timer(
        CLOCK_MONOTONIC,
        Some(&thread_id_event(
            SigNo::SIGUSR1,
            0x5449_4402,
            i32::try_from(tid).unwrap(),
        )),
    )
    .unwrap();
    release_exit_target(before_expiry);
    wait_for_closed_thread_id(tid);
    set_timer(
        timer,
        0,
        ITimerSpec {
            it_interval: ns_to_timespec(20_000_000),
            it_value: ns_to_timespec(20_000_000),
        },
        None,
    )
    .unwrap();
    sleep_ns(80_000_000);
    assert_periodic_projection(timer, 20_000_000);
    assert!(matches!(raw_sigtimedwait(SigNo::SIGUSR1, 0), Err(EAGAIN)));
    delete_timer(timer).unwrap();

    let (pending, tid) = spawn_exit_target();
    let timer = create_timer(
        CLOCK_MONOTONIC,
        Some(&thread_id_event(
            SigNo::SIGUSR1,
            0x5449_4403,
            i32::try_from(tid).unwrap(),
        )),
    )
    .unwrap();
    set_timer(
        timer,
        0,
        ITimerSpec {
            it_interval: ns_to_timespec(20_000_000),
            it_value: ns_to_timespec(20_000_000),
        },
        None,
    )
    .unwrap();
    sleep_ns(80_000_000);
    release_exit_target(pending);
    wait_for_closed_thread_id(tid);
    assert_periodic_projection(timer, 20_000_000);
    assert_eq!(get_overrun(timer), Ok(0));
    assert!(matches!(raw_sigtimedwait(SigNo::SIGUSR1, 0), Err(EAGAIN)));
    delete_timer(timer).unwrap();

    signal::sigprocmask(SigProcMaskHow::SetMask, Some(&old_mask), None).unwrap();
}

fn verify_thread_id_ignore_recovery_and_delete_after_queue() {
    let current_tid = i32::try_from(gettid().unwrap()).unwrap();
    let event = thread_id_event(SigNo::SIGUSR1, 0x5449_4404, current_tid);
    let ignore = SigAction {
        sighandler: linux_signal::SIG_IGN,
        sa_flags: 0,
        sa_restorer: core::ptr::null(),
        sa_mask: SigSet { bits: 0 },
    };
    let mut previous = empty_action();
    signal::sigaction(SigNo::SIGUSR1, Some(&ignore), Some(&mut previous)).unwrap();
    DELIVERIES.store(0, Ordering::SeqCst);
    let timer = create_timer(CLOCK_MONOTONIC, Some(&event)).unwrap();
    set_timer(
        timer,
        0,
        ITimerSpec {
            it_interval: ns_to_timespec(20_000_000),
            it_value: ns_to_timespec(20_000_000),
        },
        None,
    )
    .unwrap();
    sleep_ns(100_000_000);
    assert_eq!(DELIVERIES.load(Ordering::SeqCst), 0);
    assert_eq!(get_overrun(timer), Ok(0));
    signal::sigaction(SigNo::SIGUSR1, Some(&previous), None).unwrap();
    wait_for_deliveries(1);
    assert_eq!(LAST_TIMER_ID.load(Ordering::SeqCst), timer);
    let delivered_overrun = LAST_OVERRUN.load(Ordering::SeqCst);
    assert!(delivered_overrun > 0);
    assert_eq!(get_overrun(timer), Ok(delivered_overrun));
    delete_timer(timer).unwrap();

    let usr1 = signal_set(SigNo::SIGUSR1);
    let mut old_mask = SigSet { bits: 0 };
    signal::sigprocmask(SigProcMaskHow::Block, Some(&usr1), Some(&mut old_mask)).unwrap();
    let sigval = 0x5449_4405;
    let timer = create_timer(
        CLOCK_MONOTONIC,
        Some(&thread_id_event(SigNo::SIGUSR1, sigval, current_tid)),
    )
    .unwrap();
    set_timer(timer, 0, one_shot(20_000_000), None).unwrap();
    sleep_ns(70_000_000);
    delete_timer(timer).unwrap();

    // Deletion withdraws future enqueue authority but cannot recall the
    // occurrence already owned by Signal.
    let info = raw_sigtimedwait(SigNo::SIGUSR1, 100_000_000).unwrap();
    let fields = unsafe { info.fields.timer };
    assert_eq!(info.si_signo, SigNo::SIGUSR1.as_usize() as i32);
    assert_eq!(info.si_code, linux_signal::SI_TIMER);
    assert_eq!(fields.tid, timer);
    assert_eq!(fields.sigval.as_u64(), sigval);
    signal::sigprocmask(SigProcMaskHow::SetMask, Some(&old_mask), None).unwrap();
}

fn verify_thread_id_notification() {
    verify_thread_id_validation();
    verify_thread_id_exact_wait_and_dequeued_exit();
    verify_thread_id_exit_before_expiry_and_pending_flush();
    verify_thread_id_ignore_recovery_and_delete_after_queue();
    println!("posix-timer: SIGEV_THREAD_ID exact delivery and lifecycle checks passed");
}

fn verify_same_signal_and_overrun() {
    let usr1 = signal_set(SigNo::SIGUSR1);
    let mut old_mask = SigSet { bits: 0 };
    signal::sigprocmask(SigProcMaskHow::Block, Some(&usr1), Some(&mut old_mask)).unwrap();
    DELIVERIES.store(0, Ordering::SeqCst);
    SEEN_SIGVALS.store(0, Ordering::SeqCst);
    let first = create_timer(CLOCK_MONOTONIC, Some(&signal_event(SigNo::SIGUSR1, 1))).unwrap();
    let second = create_timer(CLOCK_MONOTONIC, Some(&signal_event(SigNo::SIGUSR1, 2))).unwrap();
    set_timer(first, 0, one_shot(20_000_000), None).unwrap();
    set_timer(second, 0, one_shot(20_000_000), None).unwrap();
    sleep_ns(60_000_000);
    signal::sigprocmask(SigProcMaskHow::SetMask, Some(&old_mask), None).unwrap();
    wait_for_deliveries(2);
    assert_eq!(SEEN_SIGVALS.load(Ordering::SeqCst) & 0b110, 0b110);
    delete_timer(first).unwrap();
    delete_timer(second).unwrap();

    let usr2 = signal_set(SigNo::SIGUSR2);
    signal::sigprocmask(SigProcMaskHow::Block, Some(&usr2), Some(&mut old_mask)).unwrap();
    DELIVERIES.store(0, Ordering::SeqCst);
    let periodic = create_timer(CLOCK_MONOTONIC, Some(&signal_event(SigNo::SIGUSR2, 7))).unwrap();
    set_timer(
        periodic,
        0,
        ITimerSpec {
            it_interval: ns_to_timespec(20_000_000),
            it_value: ns_to_timespec(20_000_000),
        },
        None,
    )
    .unwrap();
    sleep_ns(120_000_000);
    signal::sigprocmask(SigProcMaskHow::SetMask, Some(&old_mask), None).unwrap();
    wait_for_deliveries(1);
    let delivered_overrun = LAST_OVERRUN.load(Ordering::SeqCst);
    assert!(delivered_overrun >= 3);
    assert_eq!(get_overrun(periodic), Ok(delivered_overrun));
    set_timer(periodic, 0, ITimerSpec::default(), None).unwrap();
    delete_timer(periodic).unwrap();
}

fn wait_status(pid: u32, options: WaitOptions) -> WStatus {
    let option_bits = options.bits();
    loop {
        let mut status = WStatusRaw::EMPTY;
        match wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut status),
            WaitOptions::from_bits_retain(option_bits),
        ) {
            Ok(Some(waited)) => {
                assert_eq!(waited, pid);
                return status.read();
            },
            Err(EINTR) => continue,
            other => panic!("POSIX timer wait4 failed: {other:?}"),
        }
    }
}

fn verify_fork_and_unmaskable_signals() {
    let none = SigEvent {
        sigev_notify: SIGEV_NONE,
        ..SigEvent::default()
    };
    let parent_timer = create_timer(CLOCK_MONOTONIC, Some(&none)).unwrap();
    match fork().unwrap() {
        Some(pid) => assert!(matches!(
            wait_status(pid, WaitOptions::empty()),
            WStatus::Exited(0)
        )),
        None => {
            assert_eq!(get_timer(parent_timer), Err(EINVAL));
            exit(0);
        },
    }
    delete_timer(parent_timer).unwrap();

    match fork().unwrap() {
        Some(pid) => assert!(matches!(
            wait_status(pid, WaitOptions::empty()),
            WStatus::Signal(value) if value == SigNo::SIGKILL.as_usize() as i8
        )),
        None => {
            let id = create_timer(CLOCK_MONOTONIC, Some(&signal_event(SigNo::SIGKILL, 0))).unwrap();
            set_timer(id, 0, one_shot(20_000_000), None).unwrap();
            loop {
                sleep_ns(NSEC_PER_SEC);
            }
        },
    }

    match fork().unwrap() {
        Some(pid) => {
            assert!(matches!(
                wait_status(pid, WaitOptions::UNTRACED),
                WStatus::Stopped(value) if value == SigNo::SIGSTOP.as_usize() as i8
            ));
            kill(pid as i32, SigNo::SIGCONT).unwrap();
            kill(pid as i32, SigNo::SIGKILL).unwrap();
            assert!(matches!(
                wait_status(pid, WaitOptions::empty()),
                WStatus::Signal(value) if value == SigNo::SIGKILL.as_usize() as i8
            ));
        },
        None => {
            let id = create_timer(CLOCK_MONOTONIC, Some(&signal_event(SigNo::SIGSTOP, 0))).unwrap();
            set_timer(id, 0, one_shot(20_000_000), None).unwrap();
            loop {
                sleep_ns(NSEC_PER_SEC);
            }
        },
    }
}

pub(crate) fn verify_posix_timers() {
    let old_alrm = install_handler(SigNo::SIGALRM);
    let old_usr1 = install_handler(SigNo::SIGUSR1);
    let old_usr2 = install_handler(SigNo::SIGUSR2);

    verify_abi_and_fail_forward();
    verify_default_and_none();
    verify_realtime_timeline_selection();
    verify_thread_id_notification();
    verify_same_signal_and_overrun();
    verify_fork_and_unmaskable_signals();

    signal::sigaction(SigNo::SIGUSR2, Some(&old_usr2), None).unwrap();
    signal::sigaction(SigNo::SIGUSR1, Some(&old_usr1), None).unwrap();
    signal::sigaction(SigNo::SIGALRM, Some(&old_alrm), None).unwrap();
    println!("posix-timer: five syscalls, lifecycle, signal, and overrun checks passed");
}
