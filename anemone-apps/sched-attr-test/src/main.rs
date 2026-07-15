#![no_std]
#![no_main]

use core::{
    mem::size_of,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use anemone_rs::{
    abi::{
        process::linux::sched::{
            CPU_SET_WORD_BITS, CPU_SET_WORD_BYTES, CPU_SETSIZE, CpuSet, SCHED_ATTR_SIZE_VER0,
            SCHED_ATTR_SIZE_VER1, SCHED_BATCH, SCHED_DEADLINE, SCHED_FIFO, SCHED_FLAG_DL_OVERRUN,
            SCHED_FLAG_KEEP_PARAMS, SCHED_FLAG_KEEP_POLICY, SCHED_FLAG_RECLAIM,
            SCHED_FLAG_RESET_ON_FORK, SCHED_FLAG_UTIL_CLAMP_MAX, SCHED_FLAG_UTIL_CLAMP_MIN,
            SCHED_IDLE, SCHED_OTHER, SCHED_RESET_ON_FORK, SCHED_RR, SchedAttr, SchedParam,
        },
        syscall::{
            linux::{
                SYS_SCHED_GET_PRIORITY_MAX, SYS_SCHED_GET_PRIORITY_MIN, SYS_SCHED_GETAFFINITY,
                SYS_SCHED_GETATTR, SYS_SCHED_GETPARAM, SYS_SCHED_GETSCHEDULER,
                SYS_SCHED_RR_GET_INTERVAL, SYS_SCHED_SETAFFINITY, SYS_SCHED_SETATTR,
                SYS_SCHED_SETPARAM, SYS_SCHED_SETSCHEDULER, SYS_SETUID,
            },
            syscall,
        },
        time::linux::TimeSpec,
    },
    os::linux::{
        fs::{PipeFlags, close, pipe2, read, write},
        process::{
            MmapFlags, MmapProt, PriorityWhich, WStatus, WStatusRaw, WaitFor, WaitOptions, exit,
            fork, getpid, getpriority, mmap, mprotect, munmap, sched_yield, setpriority, wait4,
        },
    },
    prelude::*,
};

const MAX_WORKERS: usize = 4;
const STRESS_ROUNDS: usize = 128;
const WAIT_RETRIES: usize = 1_000_000;
const NO_TARGET: usize = usize::MAX;
const ABI_PAGE_SIZE: usize = 4096;

#[repr(C)]
struct SharedStress {
    ready: AtomicUsize,
    start: AtomicBool,
    stress_ready: AtomicUsize,
    stress_start: AtomicBool,
    pids: [AtomicUsize; MAX_WORKERS],
    cpus: [AtomicUsize; MAX_WORKERS],
    target_pids: [AtomicUsize; MAX_WORKERS],
    target_cpus: [AtomicUsize; MAX_WORKERS],
}

#[repr(C)]
struct SharedTargetScenario {
    ready: AtomicBool,
    release: AtomicBool,
}

impl SharedTargetScenario {
    const fn new() -> Self {
        Self {
            ready: AtomicBool::new(false),
            release: AtomicBool::new(false),
        }
    }
}

impl SharedStress {
    const fn new() -> Self {
        Self {
            ready: AtomicUsize::new(0),
            start: AtomicBool::new(false),
            stress_ready: AtomicUsize::new(0),
            stress_start: AtomicBool::new(false),
            pids: [
                AtomicUsize::new(0),
                AtomicUsize::new(0),
                AtomicUsize::new(0),
                AtomicUsize::new(0),
            ],
            cpus: [
                AtomicUsize::new(0),
                AtomicUsize::new(0),
                AtomicUsize::new(0),
                AtomicUsize::new(0),
            ],
            target_pids: [
                AtomicUsize::new(NO_TARGET),
                AtomicUsize::new(NO_TARGET),
                AtomicUsize::new(NO_TARGET),
                AtomicUsize::new(NO_TARGET),
            ],
            target_cpus: [
                AtomicUsize::new(0),
                AtomicUsize::new(0),
                AtomicUsize::new(0),
                AtomicUsize::new(0),
            ],
        }
    }
}

fn pid_arg(pid: i32) -> u64 {
    pid as i64 as u64
}

fn policy_arg(policy: i32) -> u64 {
    policy as i64 as u64
}

fn sched_setscheduler_raw(pid: i32, policy: i32, param: u64) -> Result<(), Errno> {
    unsafe {
        syscall(
            SYS_SCHED_SETSCHEDULER,
            pid_arg(pid),
            policy_arg(policy),
            param,
            0,
            0,
            0,
        )
    }
    .map(|_| ())
}

fn sched_setscheduler(pid: i32, policy: i32, priority: i32) -> Result<(), Errno> {
    let param = SchedParam {
        sched_priority: priority,
    };
    sched_setscheduler_raw(pid, policy, &param as *const SchedParam as u64)
}

fn sched_getscheduler(pid: i32) -> Result<i32, Errno> {
    unsafe { syscall(SYS_SCHED_GETSCHEDULER, pid_arg(pid), 0, 0, 0, 0, 0) }
        .map(|policy| policy as i32)
}

fn sched_setparam_raw(pid: i32, param: u64) -> Result<(), Errno> {
    unsafe { syscall(SYS_SCHED_SETPARAM, pid_arg(pid), param, 0, 0, 0, 0) }.map(|_| ())
}

fn sched_setparam(pid: i32, priority: i32) -> Result<(), Errno> {
    let param = SchedParam {
        sched_priority: priority,
    };
    sched_setparam_raw(pid, &param as *const SchedParam as u64)
}

fn sched_getparam_raw(pid: i32, param: u64) -> Result<(), Errno> {
    unsafe { syscall(SYS_SCHED_GETPARAM, pid_arg(pid), param, 0, 0, 0, 0) }.map(|_| ())
}

fn sched_getparam(pid: i32) -> Result<SchedParam, Errno> {
    let mut param = SchedParam::default();
    sched_getparam_raw(pid, &mut param as *mut SchedParam as u64)?;
    Ok(param)
}

fn sched_get_priority_min(policy: i32) -> Result<i32, Errno> {
    unsafe {
        syscall(
            SYS_SCHED_GET_PRIORITY_MIN,
            policy_arg(policy),
            0,
            0,
            0,
            0,
            0,
        )
    }
    .map(|priority| priority as i32)
}

fn sched_get_priority_max(policy: i32) -> Result<i32, Errno> {
    unsafe {
        syscall(
            SYS_SCHED_GET_PRIORITY_MAX,
            policy_arg(policy),
            0,
            0,
            0,
            0,
            0,
        )
    }
    .map(|priority| priority as i32)
}

fn sched_rr_get_interval_raw(pid: i32, interval: u64) -> Result<(), Errno> {
    unsafe {
        syscall(
            SYS_SCHED_RR_GET_INTERVAL,
            pid_arg(pid),
            interval,
            0,
            0,
            0,
            0,
        )
    }
    .map(|_| ())
}

fn sched_rr_get_interval(pid: i32) -> Result<TimeSpec, Errno> {
    let mut interval = TimeSpec::default();
    sched_rr_get_interval_raw(pid, &mut interval as *mut TimeSpec as u64)?;
    Ok(interval)
}

fn sched_setattr_raw(pid: i32, attr: u64, flags: u32) -> Result<(), Errno> {
    unsafe { syscall(SYS_SCHED_SETATTR, pid_arg(pid), attr, flags as u64, 0, 0, 0) }.map(|_| ())
}

fn sched_setattr(pid: i32, attr: &mut SchedAttr) -> Result<(), Errno> {
    sched_setattr_raw(pid, attr as *mut SchedAttr as u64, 0)
}

fn sched_getattr_raw(pid: i32, attr: u64, usize: usize, flags: u32) -> Result<(), Errno> {
    unsafe {
        syscall(
            SYS_SCHED_GETATTR,
            pid_arg(pid),
            attr,
            usize as u64,
            flags as u64,
            0,
            0,
        )
    }
    .map(|_| ())
}

fn sched_getattr(pid: i32, usize: usize) -> Result<SchedAttr, Errno> {
    let mut attr = SchedAttr::default();
    sched_getattr_raw(pid, &mut attr as *mut SchedAttr as u64, usize, 0)?;
    Ok(attr)
}

fn fair_attr(nice: i32, flags: u64) -> SchedAttr {
    SchedAttr {
        size: SCHED_ATTR_SIZE_VER1 as u32,
        sched_policy: SCHED_OTHER as u32,
        sched_flags: flags,
        sched_nice: nice,
        ..SchedAttr::default()
    }
}

fn rt_attr(policy: i32, priority: u32, nice: i32, flags: u64) -> SchedAttr {
    SchedAttr {
        size: SCHED_ATTR_SIZE_VER1 as u32,
        sched_policy: policy as u32,
        sched_flags: flags,
        sched_nice: nice,
        sched_priority: priority,
        ..SchedAttr::default()
    }
}

fn write_attr_prefix(buffer: &mut [u8], attr: SchedAttr) {
    assert!(buffer.len() >= SCHED_ATTR_SIZE_VER1);
    unsafe {
        core::ptr::copy_nonoverlapping(
            (&attr as *const SchedAttr).cast::<u8>(),
            buffer.as_mut_ptr(),
            SCHED_ATTR_SIZE_VER1,
        );
    }
}

fn read_attr_prefix(buffer: &[u8]) -> SchedAttr {
    assert!(buffer.len() >= SCHED_ATTR_SIZE_VER1);
    unsafe { (buffer.as_ptr() as *const SchedAttr).read_unaligned() }
}

fn sched_setaffinity_raw(pid: i32, len: usize, mask: u64) -> Result<(), Errno> {
    unsafe {
        syscall(
            SYS_SCHED_SETAFFINITY,
            pid_arg(pid),
            len as u64,
            mask,
            0,
            0,
            0,
        )
    }
    .map(|_| ())
}

fn sched_getaffinity_raw(pid: i32, len: usize, mask: u64) -> Result<usize, Errno> {
    unsafe {
        syscall(
            SYS_SCHED_GETAFFINITY,
            pid_arg(pid),
            len as u64,
            mask,
            0,
            0,
            0,
        )
    }
    .map(|bytes| bytes as usize)
}

fn sched_setaffinity(pid: i32, mask: &CpuSet) -> Result<(), Errno> {
    sched_setaffinity_raw(pid, size_of::<CpuSet>(), mask as *const CpuSet as u64)
}

fn sched_getaffinity(pid: i32) -> Result<(CpuSet, usize), Errno> {
    let mut mask = CpuSet::empty();
    let copied = sched_getaffinity_raw(pid, size_of::<CpuSet>(), &mut mask as *mut CpuSet as u64)?;
    Ok((mask, copied))
}

fn cpu_set(cpus: &[usize]) -> CpuSet {
    let mut set = CpuSet::empty();
    for cpu in cpus {
        set.set(*cpu);
    }
    set
}

fn setuid(uid: u32) -> Result<(), Errno> {
    unsafe { syscall(SYS_SETUID, uid as u64, 0, 0, 0, 0, 0) }.map(|_| ())
}

#[track_caller]
fn expect_errno<T>(result: Result<T, Errno>, expected: Errno, what: &str) {
    match result {
        Ok(_) => panic!("sched-attr-test: {what}: expected errno {expected}, got success"),
        Err(errno) => assert_eq!(errno, expected, "sched-attr-test: {what}: unexpected errno"),
    }
}

fn wait_until(mut pred: impl FnMut() -> bool, what: &str) -> Result<(), Errno> {
    for _ in 0..WAIT_RETRIES {
        if pred() {
            return Ok(());
        }
        sched_yield()?;
    }
    panic!("sched-attr-test: timed out waiting for {what}");
}

fn wait_child_exit(pid: u32) -> Result<(), Errno> {
    loop {
        let mut status = WStatusRaw::EMPTY;
        match wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut status),
            WaitOptions::empty(),
        ) {
            Ok(Some(waited)) => {
                assert_eq!(waited, pid, "sched-attr-test: waited pid mismatch");
                match status.read() {
                    WStatus::Exited(0) => return Ok(()),
                    other => panic!("sched-attr-test: worker {pid} exited unexpectedly: {other:?}"),
                }
            },
            Ok(None) => panic!("sched-attr-test: wait4 returned None without WNOHANG"),
            Err(EINTR) => {},
            Err(errno) => return Err(errno),
        }
    }
}

fn find_fixed_cpu(pid: i32) -> Result<(usize, CpuSet), Errno> {
    let (initial, copied) = sched_getaffinity(pid)?;
    assert_eq!(
        copied, CPU_SET_WORD_BYTES,
        "sched-attr-test: unexpected affinity copy size"
    );
    assert!(
        !initial.is_empty(),
        "sched-attr-test: initial affinity is empty"
    );

    for cpu in 0..CPU_SETSIZE {
        let singleton = cpu_set(&[cpu]);
        if initial.contains(cpu) && sched_setaffinity(pid, &singleton).is_ok() {
            let (saved, copied) = sched_getaffinity(pid)?;
            assert_eq!(copied, CPU_SET_WORD_BYTES);
            assert_eq!(
                saved, singleton,
                "sched-attr-test: singleton affinity did not round-trip"
            );
            return Ok((cpu, initial));
        }
    }
    panic!("sched-attr-test: no allowed singleton matched the fixed owner CPU");
}

fn test_local_affinity() -> Result<CpuSet, Errno> {
    println!("sched-attr-test: CASE local-affinity start");
    let (cpu, initial) = find_fixed_cpu(0)?;
    let singleton = cpu_set(&[cpu]);

    let short_len = cpu / 8 + 1;
    assert!(short_len < CPU_SET_WORD_BYTES);
    sched_setaffinity_raw(0, short_len, &singleton as *const CpuSet as u64)?;
    assert_eq!(sched_getaffinity(0)?.0, singleton);

    sched_setaffinity(0, &CpuSet::full())?;
    let normalized = sched_getaffinity(0)?.0;
    println!(
        "sched-attr-test: normalization probe cpu={cpu} initial-count={} online-count={}",
        initial.count(),
        normalized.count(),
    );
    assert!(
        normalized.contains(cpu),
        "sched-attr-test: normalized online mask excluded the fixed owner"
    );

    let mut request = singleton;
    request.set(CPU_SET_WORD_BITS);
    sched_setaffinity(0, &request)?;
    let mut output = CpuSet::full();
    let copied = sched_getaffinity_raw(0, size_of::<CpuSet>(), &mut output as *mut CpuSet as u64)?;
    assert_eq!(copied, CPU_SET_WORD_BYTES);
    for candidate in 0..CPU_SET_WORD_BITS {
        assert_eq!(output.contains(candidate), candidate == cpu);
    }
    assert!(
        output.contains(CPU_SET_WORD_BITS),
        "sched-attr-test: getter overwrote bytes beyond its raw return length"
    );

    sched_setaffinity(0, &normalized)?;
    println!(
        "sched-attr-test: CASE local-affinity ok cpu={cpu} count={}",
        normalized.count()
    );
    Ok(normalized)
}

fn test_errno_ordering() -> Result<(), Errno> {
    println!("sched-attr-test: CASE errno-ordering start");
    let missing = i32::MAX;
    expect_errno(
        sched_setaffinity_raw(missing, size_of::<CpuSet>(), u64::MAX),
        EFAULT,
        "set bad input before missing target",
    );
    expect_errno(
        sched_getaffinity_raw(missing, 1, u64::MAX),
        EINVAL,
        "get invalid len before missing target",
    );
    expect_errno(
        sched_getaffinity_raw(missing, size_of::<CpuSet>(), u64::MAX),
        ESRCH,
        "get missing target before bad output",
    );
    expect_errno(
        sched_setaffinity_raw(0, 0, u64::MAX),
        EINVAL,
        "zero-length set must not touch the pointer",
    );
    expect_errno(
        sched_getaffinity_raw(0, CPU_SET_WORD_BYTES - 1, u64::MAX),
        EINVAL,
        "get short or unaligned length",
    );
    println!("sched-attr-test: CASE errno-ordering ok");
    Ok(())
}

fn test_legacy_policy_matrix() -> Result<(), Errno> {
    println!("sched-attr-test: CASE legacy-policy start");

    for policy in [SCHED_OTHER, SCHED_BATCH, SCHED_IDLE, SCHED_DEADLINE] {
        assert_eq!(sched_get_priority_min(policy)?, 0);
        assert_eq!(sched_get_priority_max(policy)?, 0);
    }
    for policy in [SCHED_FIFO, SCHED_RR] {
        assert_eq!(sched_get_priority_min(policy)?, 1);
        assert_eq!(sched_get_priority_max(policy)?, 99);
    }
    expect_errno(
        sched_get_priority_min(SCHED_FIFO | SCHED_RESET_ON_FORK),
        EINVAL,
        "priority query rejects reset encoding",
    );

    sched_setscheduler(0, SCHED_OTHER, 0)?;
    for policy in [SCHED_BATCH, SCHED_IDLE, SCHED_DEADLINE] {
        expect_errno(
            sched_setscheduler(0, policy, 0),
            EINVAL,
            "unsupported policy setter",
        );
    }
    assert_eq!(sched_getscheduler(0)?, SCHED_OTHER);
    assert_eq!(sched_getparam(0)?.sched_priority, 0);
    let fair_interval = sched_rr_get_interval(0)?;
    assert!(
        fair_interval.tv_sec > 0 || fair_interval.tv_nsec > 0,
        "sched-attr-test: Fair interval must be one effective tick"
    );

    sched_setscheduler(0, SCHED_FIFO | SCHED_RESET_ON_FORK, 20)?;
    assert_eq!(sched_getscheduler(0)?, SCHED_FIFO | SCHED_RESET_ON_FORK);
    assert_eq!(sched_getparam(0)?.sched_priority, 20);
    assert_eq!(sched_rr_get_interval(0)?, TimeSpec::default());

    sched_setparam(0, 10)?;
    assert_eq!(sched_getparam(0)?.sched_priority, 10);
    sched_setparam(0, 30)?;
    assert_eq!(sched_getparam(0)?.sched_priority, 30);

    sched_setscheduler(0, SCHED_RR, 40)?;
    assert_eq!(sched_getscheduler(0)?, SCHED_RR);
    assert_eq!(sched_getparam(0)?.sched_priority, 40);
    let rr_interval = sched_rr_get_interval(0)?;
    assert!(
        rr_interval.tv_sec > 0 || rr_interval.tv_nsec > 0,
        "sched-attr-test: RR full quantum must be non-zero"
    );
    assert!(
        (rr_interval.tv_sec, rr_interval.tv_nsec) >= (fair_interval.tv_sec, fair_interval.tv_nsec),
        "sched-attr-test: RR full quantum must cover at least one Fair tick"
    );

    sched_setscheduler(0, SCHED_FIFO, 35)?;
    assert_eq!(sched_getscheduler(0)?, SCHED_FIFO);
    sched_setscheduler(0, SCHED_RR, 35)?;
    assert_eq!(sched_getscheduler(0)?, SCHED_RR);
    sched_setscheduler(0, SCHED_OTHER, 0)?;
    assert_eq!(sched_getscheduler(0)?, SCHED_OTHER);

    println!("sched-attr-test: CASE legacy-policy ok");
    Ok(())
}

fn test_legacy_policy_errno_ordering() -> Result<(), Errno> {
    println!("sched-attr-test: CASE legacy-errno start");
    let missing = i32::MAX;
    let invalid = SchedParam {
        sched_priority: 100,
    };

    expect_errno(
        sched_setscheduler_raw(missing, -1, u64::MAX),
        EINVAL,
        "setscheduler negative policy before pointer and target",
    );
    expect_errno(
        sched_setscheduler_raw(missing, SCHED_OTHER, u64::MAX),
        EFAULT,
        "setscheduler copy before missing target",
    );
    expect_errno(
        sched_setscheduler(missing, SCHED_BATCH, 0),
        ESRCH,
        "setscheduler missing target before unsupported policy",
    );
    expect_errno(
        sched_setparam_raw(missing, &invalid as *const SchedParam as u64),
        ESRCH,
        "setparam missing target before bad range",
    );
    expect_errno(
        sched_getparam_raw(missing, 0),
        EINVAL,
        "getparam null output before target",
    );
    expect_errno(
        sched_getparam_raw(missing, u64::MAX),
        ESRCH,
        "getparam missing target before output access",
    );
    expect_errno(
        sched_rr_get_interval_raw(missing, u64::MAX),
        ESRCH,
        "RR interval missing target before bad output",
    );

    let mut unaligned_param = [0u8; size_of::<SchedParam>() + 1];
    unaligned_param[1..].copy_from_slice(&0i32.to_ne_bytes());
    sched_setscheduler_raw(0, SCHED_OTHER, unsafe { unaligned_param.as_ptr().add(1) }
        as u64)?;
    unaligned_param[1..].fill(u8::MAX);
    sched_getparam_raw(0, unsafe { unaligned_param.as_mut_ptr().add(1) } as u64)?;
    assert_eq!(
        i32::from_ne_bytes(unaligned_param[1..].try_into().unwrap()),
        0
    );

    let mut unaligned_interval = [0u8; size_of::<TimeSpec>() + 1];
    sched_rr_get_interval_raw(0, unsafe { unaligned_interval.as_mut_ptr().add(1) } as u64)?;
    let seconds = i64::from_ne_bytes(unaligned_interval[1..9].try_into().unwrap());
    let nanos = i64::from_ne_bytes(unaligned_interval[9..17].try_into().unwrap());
    assert!(seconds > 0 || nanos > 0);

    println!("sched-attr-test: CASE legacy-errno ok");
    Ok(())
}

fn test_legacy_policy_permission_ordering() -> Result<(), Errno> {
    println!("sched-attr-test: CASE legacy-permission start");
    let parent = getpid()?;
    sched_setscheduler(0, SCHED_OTHER, 0)?;
    match fork()? {
        Some(child) => wait_child_exit(child)?,
        None => {
            sched_setscheduler(0, SCHED_OTHER | SCHED_RESET_ON_FORK, 0)
                .expect("sched-attr-test: failed to arm reset before dropping privilege");
            setuid(1000).expect("sched-attr-test: setuid failed in Fair permission child");
            expect_errno(
                sched_setscheduler(0, SCHED_OTHER, 0),
                EPERM,
                "unprivileged caller cannot clear armed reset",
            );
            expect_errno(
                sched_setparam(parent as i32, 1),
                EINVAL,
                "Fair family mismatch before permission",
            );
            expect_errno(
                sched_setparam(parent as i32, 0),
                EPERM,
                "Fair family match permission denial",
            );
            expect_errno(
                sched_setscheduler(parent as i32, SCHED_OTHER, 0),
                EPERM,
                "setscheduler permission denial",
            );
            exit(0)
        },
    }

    sched_setscheduler(0, SCHED_FIFO, 20)?;
    match fork()? {
        Some(child) => wait_child_exit(child)?,
        None => {
            setuid(1000).expect("sched-attr-test: setuid failed in RT permission child");
            expect_errno(
                sched_setparam(parent as i32, 0),
                EINVAL,
                "RT family mismatch before permission",
            );
            expect_errno(
                sched_setparam(parent as i32, 10),
                EPERM,
                "RT family match permission denial",
            );
            exit(0)
        },
    }
    sched_setscheduler(0, SCHED_OTHER, 0)?;
    println!("sched-attr-test: CASE legacy-permission ok");
    Ok(())
}

fn test_attr_size_and_tail_matrix() -> Result<(), Errno> {
    println!("sched-attr-test: CASE attr-size-tail start");
    sched_setscheduler(0, SCHED_OTHER, 0)?;

    let mut size_zero = fair_attr(-3, 0);
    size_zero.size = 0;
    sched_setattr(0, &mut size_zero)?;
    assert_eq!(sched_getattr(0, SCHED_ATTR_SIZE_VER1)?.sched_nice, -3);

    for size in [
        SCHED_ATTR_SIZE_VER0,
        SCHED_ATTR_SIZE_VER1 - 1,
        SCHED_ATTR_SIZE_VER1,
    ] {
        let mut attr = fair_attr(size as i32 - 60, 0);
        attr.size = size as u32;
        sched_setattr(0, &mut attr)?;
        assert_eq!(
            sched_getattr(0, SCHED_ATTR_SIZE_VER1)?.sched_nice,
            (size as i32 - 60).clamp(-20, 19),
        );
    }

    for size in [SCHED_ATTR_SIZE_VER1 + 1, ABI_PAGE_SIZE] {
        let mut attr = fair_attr(4, 0);
        attr.size = size as u32;
        let mut future = vec![0u8; size];
        write_attr_prefix(&mut future, attr);
        sched_setattr_raw(0, future.as_mut_ptr() as u64, 0)?;
        assert_eq!(sched_getattr(0, SCHED_ATTR_SIZE_VER1)?.sched_nice, 4);
    }
    let before_failures = sched_getattr(0, SCHED_ATTR_SIZE_VER1)?;

    let missing = i32::MAX;
    let mut too_short = fair_attr(0, 0);
    too_short.size = (SCHED_ATTR_SIZE_VER0 - 1) as u32;
    expect_errno(
        sched_setattr(missing, &mut too_short),
        E2BIG,
        "setattr invalid size before missing target",
    );
    assert_eq!(too_short.size, SCHED_ATTR_SIZE_VER1 as u32);

    let mut too_long = fair_attr(0, 0);
    too_long.size = (ABI_PAGE_SIZE + 1) as u32;
    expect_errno(
        sched_setattr(0, &mut too_long),
        E2BIG,
        "setattr size above PAGE_SIZE",
    );
    assert_eq!(too_long.size, SCHED_ATTR_SIZE_VER1 as u32);

    let read_only = mmap(
        0,
        ABI_PAGE_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )?;
    let mut invalid = fair_attr(0, 0);
    invalid.size = (SCHED_ATTR_SIZE_VER0 - 1) as u32;
    unsafe { read_only.as_ptr().cast::<SchedAttr>().write(invalid) };
    mprotect(read_only.as_ptr(), ABI_PAGE_SIZE, MmapProt::PROT_READ)?;
    expect_errno(
        sched_setattr_raw(0, read_only.as_ptr() as u64, 0),
        E2BIG,
        "setattr failed size write-back preserves E2BIG",
    );
    mprotect(
        read_only.as_ptr(),
        ABI_PAGE_SIZE,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
    )?;
    assert_eq!(
        unsafe { read_only.as_ptr().cast::<SchedAttr>().read() }.size,
        (SCHED_ATTR_SIZE_VER0 - 1) as u32,
    );
    munmap(read_only.as_ptr(), ABI_PAGE_SIZE)?;

    let mut attr = fair_attr(0, 0);
    attr.size = (SCHED_ATTR_SIZE_VER1 + 1) as u32;
    let mut nonzero_tail = vec![0u8; SCHED_ATTR_SIZE_VER1 + 1];
    write_attr_prefix(&mut nonzero_tail, attr);
    nonzero_tail[SCHED_ATTR_SIZE_VER1] = 1;
    expect_errno(
        sched_setattr_raw(0, nonzero_tail.as_mut_ptr() as u64, 0),
        E2BIG,
        "setattr nonzero future tail",
    );
    assert_eq!(
        read_attr_prefix(&nonzero_tail).size,
        SCHED_ATTR_SIZE_VER1 as u32,
        "setattr E2BIG must advertise known size",
    );

    let mapping = mmap(
        0,
        ABI_PAGE_SIZE * 2,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )?;
    let base = mapping.as_ptr();
    let tail_fault_ptr = unsafe { base.add(ABI_PAGE_SIZE - SCHED_ATTR_SIZE_VER1) };
    let mut tail_fault = fair_attr(0, 0);
    tail_fault.size = (SCHED_ATTR_SIZE_VER1 + 1) as u32;
    unsafe {
        tail_fault_ptr
            .cast::<SchedAttr>()
            .write_unaligned(tail_fault)
    };
    munmap(unsafe { base.add(ABI_PAGE_SIZE) }, ABI_PAGE_SIZE)?;
    expect_errno(
        sched_setattr_raw(0, tail_fault_ptr as u64, 0),
        EFAULT,
        "setattr inaccessible future tail",
    );
    munmap(base, ABI_PAGE_SIZE)?;

    assert_eq!(
        sched_getattr(0, SCHED_ATTR_SIZE_VER1)?,
        before_failures,
        "setattr size/tail failures must not publish partial config",
    );

    println!("sched-attr-test: CASE attr-size-tail ok");
    Ok(())
}

fn test_attr_errno_ordering_and_failure_atomicity() -> Result<(), Errno> {
    println!("sched-attr-test: CASE attr-errno start");
    let missing = i32::MAX;
    let mut baseline = fair_attr(-6, SCHED_FLAG_RESET_ON_FORK);
    sched_setattr(0, &mut baseline)?;
    let before = sched_getattr(0, SCHED_ATTR_SIZE_VER1)?;

    expect_errno(
        sched_setattr_raw(missing, 0, 0),
        EINVAL,
        "setattr null before target",
    );
    let mut valid = fair_attr(0, 0);
    expect_errno(
        sched_setattr_raw(missing, &mut valid as *mut SchedAttr as u64, 1),
        EINVAL,
        "setattr syscall flags before copy and target",
    );
    expect_errno(
        sched_setattr_raw(-1, &mut valid as *mut SchedAttr as u64, 0),
        EINVAL,
        "setattr negative pid before copy",
    );
    expect_errno(
        sched_setattr_raw(missing, u64::MAX, 0),
        EFAULT,
        "setattr size read before target",
    );

    let mut short_util = fair_attr(0, SCHED_FLAG_UTIL_CLAMP_MIN);
    short_util.size = SCHED_ATTR_SIZE_VER0 as u32;
    expect_errno(
        sched_setattr(missing, &mut short_util),
        EINVAL,
        "setattr util field presence before target",
    );

    let mut negative_policy = fair_attr(0, 0);
    negative_policy.sched_policy = u32::MAX;
    expect_errno(
        sched_setattr(missing, &mut negative_policy),
        EINVAL,
        "setattr signed policy sanity before target",
    );

    let mut unsupported_policy = fair_attr(0, 0);
    unsupported_policy.sched_policy = SCHED_DEADLINE as u32;
    expect_errno(
        sched_setattr(missing, &mut unsupported_policy),
        ESRCH,
        "setattr missing target before unsupported policy",
    );
    let mut unsupported_flag = fair_attr(0, SCHED_FLAG_KEEP_PARAMS);
    expect_errno(
        sched_setattr(missing, &mut unsupported_flag),
        ESRCH,
        "setattr missing target before unsupported attr flag",
    );

    expect_errno(
        sched_getattr_raw(missing, 0, SCHED_ATTR_SIZE_VER1, 0),
        EINVAL,
        "getattr null before target",
    );
    expect_errno(
        sched_getattr_raw(missing, u64::MAX, SCHED_ATTR_SIZE_VER0 - 1, 0),
        EINVAL,
        "getattr invalid usize before target",
    );
    expect_errno(
        sched_getattr_raw(missing, u64::MAX, ABI_PAGE_SIZE + 1, 0),
        EINVAL,
        "getattr usize above PAGE_SIZE before target",
    );
    expect_errno(
        sched_getattr_raw(-1, u64::MAX, SCHED_ATTR_SIZE_VER1, 0),
        EINVAL,
        "getattr negative pid before target",
    );
    expect_errno(
        sched_getattr_raw(missing, u64::MAX, SCHED_ATTR_SIZE_VER1, 0),
        ESRCH,
        "getattr missing target before output access",
    );
    expect_errno(
        sched_getattr_raw(0, u64::MAX, SCHED_ATTR_SIZE_VER1, 0),
        EFAULT,
        "getattr existing target bad output",
    );
    expect_errno(
        sched_getattr_raw(0, u64::MAX, SCHED_ATTR_SIZE_VER1, 1),
        EINVAL,
        "getattr syscall flags before output access",
    );
    let mut truncated = SchedAttr::default();
    sched_getattr_raw(
        0,
        &mut truncated as *mut SchedAttr as u64,
        (1usize << 32) | SCHED_ATTR_SIZE_VER1,
        0,
    )?;
    assert_eq!(
        truncated.size, SCHED_ATTR_SIZE_VER1 as u32,
        "getattr unsigned-int size must ignore register high bits",
    );

    for flag in [
        SCHED_FLAG_RECLAIM,
        SCHED_FLAG_DL_OVERRUN,
        SCHED_FLAG_KEEP_POLICY,
        SCHED_FLAG_KEEP_PARAMS,
        SCHED_FLAG_UTIL_CLAMP_MIN,
        SCHED_FLAG_UTIL_CLAMP_MAX,
        1 << 63,
    ] {
        let mut attr = fair_attr(0, flag);
        expect_errno(
            sched_setattr(0, &mut attr),
            EINVAL,
            "setattr unsupported feature flag",
        );
    }
    for policy in [SCHED_BATCH, SCHED_IDLE, SCHED_DEADLINE, 99] {
        let mut attr = fair_attr(0, 0);
        attr.sched_policy = policy as u32;
        expect_errno(
            sched_setattr(0, &mut attr),
            EINVAL,
            "setattr unsupported policy",
        );
    }
    let mut bad_priority = fair_attr(0, 0);
    bad_priority.sched_priority = 1;
    expect_errno(
        sched_setattr(0, &mut bad_priority),
        EINVAL,
        "setattr invalid Fair priority",
    );

    assert_eq!(
        sched_getattr(0, SCHED_ATTR_SIZE_VER1)?,
        before,
        "setattr failures must not publish partial config",
    );
    println!("sched-attr-test: CASE attr-errno ok");
    Ok(())
}

fn test_attr_projection_and_full_output_range() -> Result<(), Errno> {
    println!("sched-attr-test: CASE attr-projection start");
    let mut fair = fair_attr(-7, SCHED_FLAG_RESET_ON_FORK);
    fair.sched_runtime = 1;
    fair.sched_deadline = 2;
    fair.sched_period = 3;
    fair.sched_util_min = 4;
    fair.sched_util_max = 5;
    sched_setattr(0, &mut fair)?;

    for usize in [
        SCHED_ATTR_SIZE_VER0,
        SCHED_ATTR_SIZE_VER1 - 1,
        SCHED_ATTR_SIZE_VER1,
    ] {
        let output = sched_getattr(0, usize)?;
        assert_eq!(output.size, usize as u32);
        assert_eq!(output.sched_policy, SCHED_OTHER as u32);
        assert_eq!(output.sched_flags, SCHED_FLAG_RESET_ON_FORK);
        assert_eq!(output.sched_nice, -7);
        assert_eq!(output.sched_priority, 0);
        assert_eq!(
            (
                output.sched_runtime,
                output.sched_deadline,
                output.sched_period,
                output.sched_util_min,
                output.sched_util_max,
            ),
            (0, 0, 0, 0, 0),
        );
    }

    let mut future_output = vec![0xa5u8; ABI_PAGE_SIZE];
    sched_getattr_raw(0, future_output.as_mut_ptr() as u64, ABI_PAGE_SIZE, 0)?;
    let projected = read_attr_prefix(&future_output);
    assert_eq!(projected.size, SCHED_ATTR_SIZE_VER1 as u32);
    assert_eq!(projected.sched_nice, -7);
    assert!(
        future_output[SCHED_ATTR_SIZE_VER1..]
            .iter()
            .all(|byte| *byte == 0xa5),
        "getattr must preserve a future userspace tail",
    );

    let mapping = mmap(
        0,
        ABI_PAGE_SIZE * 2,
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_PRIVATE | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )?;
    let base = mapping.as_ptr();
    let output_ptr = unsafe { base.add(ABI_PAGE_SIZE - SCHED_ATTR_SIZE_VER1) };
    unsafe { core::ptr::write_bytes(output_ptr, 0xa5, SCHED_ATTR_SIZE_VER1) };
    munmap(unsafe { base.add(ABI_PAGE_SIZE) }, ABI_PAGE_SIZE)?;
    expect_errno(
        sched_getattr_raw(0, output_ptr as u64, SCHED_ATTR_SIZE_VER1 + 1, 0),
        EFAULT,
        "getattr validates full future output range",
    );
    assert!(
        unsafe { core::slice::from_raw_parts(output_ptr, SCHED_ATTR_SIZE_VER1) }
            .iter()
            .all(|byte| *byte == 0xa5),
        "getattr full-range failure must not partially overwrite the prefix",
    );
    munmap(base, ABI_PAGE_SIZE)?;

    assert_eq!(getpriority(PriorityWhich::Process, 0)?, -7);
    let mut fifo = rt_attr(SCHED_FIFO, 42, 19, SCHED_FLAG_RESET_ON_FORK);
    fifo.sched_runtime = 11;
    fifo.sched_deadline = 12;
    fifo.sched_period = 13;
    sched_setattr(0, &mut fifo)?;
    let output = sched_getattr(0, SCHED_ATTR_SIZE_VER1)?;
    assert_eq!(output.sched_policy, SCHED_FIFO as u32);
    assert_eq!(output.sched_flags, SCHED_FLAG_RESET_ON_FORK);
    assert_eq!(output.sched_nice, 0);
    assert_eq!(output.sched_priority, 42);
    assert_eq!(getpriority(PriorityWhich::Process, 0)?, -7);

    let mut rr = rt_attr(SCHED_RR, 31, 19, 0);
    sched_setattr(0, &mut rr)?;
    let output = sched_getattr(0, SCHED_ATTR_SIZE_VER1)?;
    assert_eq!(output.sched_policy, SCHED_RR as u32);
    assert_eq!(output.sched_nice, 0);
    assert_eq!(output.sched_priority, 31);
    assert_eq!(getpriority(PriorityWhich::Process, 0)?, -7);

    let mut fair = fair_attr(3, 0);
    sched_setattr(0, &mut fair)?;
    assert_eq!(sched_getattr(0, SCHED_ATTR_SIZE_VER1)?.sched_nice, 3);
    println!("sched-attr-test: CASE attr-projection ok");
    Ok(())
}

fn test_attr_permission_ordering() -> Result<(), Errno> {
    println!("sched-attr-test: CASE attr-permission start");
    let parent = getpid()?;
    match fork()? {
        Some(child) => wait_child_exit(child)?,
        None => {
            let mut armed = fair_attr(0, SCHED_FLAG_RESET_ON_FORK);
            sched_setattr(0, &mut armed)
                .expect("sched-attr-test: failed to arm attr reset before setuid");
            setuid(1000).expect("sched-attr-test: setuid failed in attr permission child");

            let mut clear = fair_attr(0, 0);
            expect_errno(
                sched_setattr(0, &mut clear),
                EPERM,
                "unprivileged setattr cannot clear armed reset",
            );
            let mut invalid = fair_attr(0, 0);
            invalid.sched_priority = 1;
            expect_errno(
                sched_setattr(parent as i32, &mut invalid),
                EINVAL,
                "setattr policy validation before permission",
            );
            let mut valid = fair_attr(0, 0);
            expect_errno(
                sched_setattr(parent as i32, &mut valid),
                EPERM,
                "setattr permission denial for existing target",
            );
            exit(0)
        },
    }
    println!("sched-attr-test: CASE attr-permission ok");
    Ok(())
}

fn test_external_runnable_policy_target() -> Result<(), Errno> {
    println!("sched-attr-test: CASE runnable-policy-target start");
    let mapping = mmap(
        0,
        size_of::<SharedTargetScenario>(),
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_SHARED | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )?;
    let shared_ptr = mapping.as_ptr().cast::<SharedTargetScenario>();
    unsafe { shared_ptr.write(SharedTargetScenario::new()) };
    let shared = unsafe { &*shared_ptr };

    match fork()? {
        Some(child) => {
            wait_until(
                || shared.ready.load(Ordering::Acquire),
                "runnable policy target",
            )?;
            sched_setscheduler(child as i32, SCHED_OTHER | SCHED_RESET_ON_FORK, 0)?;
            assert_eq!(
                sched_getscheduler(child as i32)?,
                SCHED_OTHER | SCHED_RESET_ON_FORK
            );
            sched_setparam(child as i32, 0)?;
            assert_eq!(sched_getparam(child as i32)?.sched_priority, 0);
            shared.release.store(true, Ordering::Release);
            wait_child_exit(child)?;
        },
        None => {
            shared.ready.store(true, Ordering::Release);
            wait_until(
                || shared.release.load(Ordering::Acquire),
                "runnable target release",
            )
            .expect("sched-attr-test: runnable child wait failed");
            assert_eq!(
                sched_getscheduler(0).unwrap(),
                SCHED_OTHER | SCHED_RESET_ON_FORK
            );
            assert_eq!(sched_getparam(0).unwrap().sched_priority, 0);
            exit(0)
        },
    }
    munmap(mapping.as_ptr(), size_of::<SharedTargetScenario>())?;
    println!(
        "sched-attr-test: CASE runnable-policy-target ok (functional external target scenario)"
    );
    Ok(())
}

fn test_pipe_blocked_policy_target() -> Result<(), Errno> {
    println!("sched-attr-test: CASE pipe-blocked-policy-target start");
    let (read_fd, write_fd) = pipe2(PipeFlags::empty())?;
    let mapping = mmap(
        0,
        size_of::<SharedTargetScenario>(),
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_SHARED | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )?;
    let shared_ptr = mapping.as_ptr().cast::<SharedTargetScenario>();
    unsafe { shared_ptr.write(SharedTargetScenario::new()) };
    let shared = unsafe { &*shared_ptr };

    match fork()? {
        Some(child) => {
            close(read_fd)?;
            wait_until(
                || shared.ready.load(Ordering::Acquire),
                "pipe-blocked policy target",
            )?;
            for _ in 0..32 {
                sched_yield()?;
            }
            sched_setscheduler(child as i32, SCHED_RR, 30)?;
            assert_eq!(sched_getscheduler(child as i32)?, SCHED_RR);
            sched_setparam(child as i32, 15)?;
            assert_eq!(sched_getparam(child as i32)?.sched_priority, 15);
            assert_eq!(write(write_fd, &[1])?, 1);
            close(write_fd)?;
            wait_child_exit(child)?;
        },
        None => {
            close(write_fd).expect("sched-attr-test: blocked child close writer failed");
            shared.ready.store(true, Ordering::Release);
            let mut byte = [0u8; 1];
            assert_eq!(
                read(read_fd, &mut byte).expect("sched-attr-test: blocked child read failed"),
                1
            );
            assert_eq!(sched_getscheduler(0).unwrap(), SCHED_RR);
            assert_eq!(sched_getparam(0).unwrap().sched_priority, 15);
            close(read_fd).expect("sched-attr-test: blocked child close reader failed");
            exit(0)
        },
    }
    munmap(mapping.as_ptr(), size_of::<SharedTargetScenario>())?;
    println!(
        "sched-attr-test: CASE pipe-blocked-policy-target ok (functional blocked target scenario)"
    );
    Ok(())
}

fn test_permission_precedes_mask_semantics() -> Result<(), Errno> {
    println!("sched-attr-test: CASE permission-ordering start");
    let parent = getpid()?;
    match fork()? {
        Some(child) => wait_child_exit(child)?,
        None => {
            setuid(1000).expect("sched-attr-test: setuid failed in permission child");
            expect_errno(
                sched_setaffinity(parent as i32, &CpuSet::empty()),
                EPERM,
                "permission must precede empty-mask semantics",
            );
            exit(0)
        },
    }
    println!("sched-attr-test: CASE permission-ordering ok");
    Ok(())
}

fn stress_worker(shared: &SharedStress, index: usize) -> ! {
    let (cpu, _) = find_fixed_cpu(0).expect("sched-attr-test: worker could not identify owner CPU");
    shared.pids[index].store(getpid().unwrap() as usize, Ordering::Release);
    shared.cpus[index].store(cpu + 1, Ordering::Release);
    shared.ready.fetch_add(1, Ordering::AcqRel);

    wait_until(|| shared.start.load(Ordering::Acquire), "worker assignment")
        .expect("sched-attr-test: worker assignment wait failed");
    let target_pid = shared.target_pids[index].load(Ordering::Acquire);
    if target_pid == NO_TARGET {
        exit(0)
    }
    let target_cpu = shared.target_cpus[index].load(Ordering::Acquire) - 1;
    assert_ne!(cpu, target_cpu);

    shared.stress_ready.fetch_add(1, Ordering::AcqRel);
    wait_until(
        || shared.stress_start.load(Ordering::Acquire),
        "mutual remote stress barrier",
    )
    .expect("sched-attr-test: stress barrier wait failed");

    let both = cpu_set(&[cpu, target_cpu]);
    for round in 0..STRESS_ROUNDS {
        let nice = (round & 1) as i32;
        setpriority(PriorityWhich::Process, target_pid as i32, nice)
            .expect("sched-attr-test: remote priority setter failed");
        assert_eq!(
            getpriority(PriorityWhich::Process, target_pid as i32)
                .expect("sched-attr-test: remote priority getter failed"),
            nice,
            "sched-attr-test: remote priority read-back mismatch"
        );

        let requested = if round & 1 == 0 {
            cpu_set(&[target_cpu])
        } else {
            both
        };
        sched_setaffinity(target_pid as i32, &requested)
            .expect("sched-attr-test: remote affinity setter failed");
        assert_eq!(
            sched_getaffinity(target_pid as i32)
                .expect("sched-attr-test: remote affinity getter failed")
                .0,
            requested,
            "sched-attr-test: remote affinity read-back mismatch"
        );
    }
    exit(0)
}

fn test_remote_gate_stress(initial_mask: CpuSet) -> Result<(), Errno> {
    if initial_mask.count() < 2 {
        println!("sched-attr-test: CASE remote-gate-stress SKIP single CPU");
        return Ok(());
    }

    println!("sched-attr-test: CASE remote-gate-stress start");
    let mapping = mmap(
        0,
        size_of::<SharedStress>(),
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_SHARED | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )?;
    let shared_ptr = mapping.as_ptr().cast::<SharedStress>();
    unsafe { shared_ptr.write(SharedStress::new()) };
    let shared = unsafe { &*shared_ptr };

    let mut children = Vec::with_capacity(MAX_WORKERS);
    for index in 0..MAX_WORKERS {
        match fork()? {
            Some(pid) => children.push(pid),
            None => stress_worker(shared, index),
        }
    }

    wait_until(
        || shared.ready.load(Ordering::Acquire) == MAX_WORKERS,
        "worker owner discovery",
    )?;
    let mut pair = None;
    for first in 0..MAX_WORKERS {
        let first_cpu = shared.cpus[first].load(Ordering::Acquire) - 1;
        for second in first + 1..MAX_WORKERS {
            let second_cpu = shared.cpus[second].load(Ordering::Acquire) - 1;
            if first_cpu != second_cpu {
                pair = Some((first, second, first_cpu, second_cpu));
                break;
            }
        }
        if pair.is_some() {
            break;
        }
    }
    let (first, second, first_cpu, second_cpu) =
        pair.expect("sched-attr-test: could not place workers on two owner CPUs");
    let first_pid = shared.pids[first].load(Ordering::Acquire);
    let second_pid = shared.pids[second].load(Ordering::Acquire);
    shared.target_pids[first].store(second_pid, Ordering::Release);
    shared.target_cpus[first].store(second_cpu + 1, Ordering::Release);
    shared.target_pids[second].store(first_pid, Ordering::Release);
    shared.target_cpus[second].store(first_cpu + 1, Ordering::Release);
    shared.start.store(true, Ordering::Release);

    wait_until(
        || shared.stress_ready.load(Ordering::Acquire) == 2,
        "selected workers at stress barrier",
    )?;
    shared.stress_start.store(true, Ordering::Release);

    for child in children {
        wait_child_exit(child)?;
    }
    munmap(mapping.as_ptr(), size_of::<SharedStress>())?;
    println!(
        "sched-attr-test: CASE remote-gate-stress ok cpus=({first_cpu},{second_cpu}) rounds={STRESS_ROUNDS}"
    );
    Ok(())
}

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    println!("sched-attr-test: BEGIN");
    let initial_mask = test_local_affinity()?;
    test_errno_ordering()?;
    test_permission_precedes_mask_semantics()?;
    test_legacy_policy_matrix()?;
    test_legacy_policy_errno_ordering()?;
    test_legacy_policy_permission_ordering()?;
    test_attr_size_and_tail_matrix()?;
    test_attr_errno_ordering_and_failure_atomicity()?;
    test_attr_projection_and_full_output_range()?;
    test_attr_permission_ordering()?;
    test_external_runnable_policy_target()?;
    test_pipe_blocked_policy_target()?;
    test_remote_gate_stress(initial_mask)?;
    println!("sched-attr-test: END all available cases passed");
    Ok(())
}
