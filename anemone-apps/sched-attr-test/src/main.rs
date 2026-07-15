#![no_std]
#![no_main]

use core::{
    mem::size_of,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use anemone_rs::{
    abi::{
        process::linux::sched::{CPU_SET_WORD_BITS, CPU_SET_WORD_BYTES, CPU_SETSIZE, CpuSet},
        syscall::{
            linux::{SYS_SCHED_GETAFFINITY, SYS_SCHED_SETAFFINITY, SYS_SETUID},
            syscall,
        },
    },
    os::linux::process::{
        MmapFlags, MmapProt, PriorityWhich, WStatus, WStatusRaw, WaitFor, WaitOptions, exit, fork,
        getpid, getpriority, mmap, munmap, sched_yield, setpriority, wait4,
    },
    prelude::*,
};

const MAX_WORKERS: usize = 4;
const STRESS_ROUNDS: usize = 128;
const WAIT_RETRIES: usize = 1_000_000;
const NO_TARGET: usize = usize::MAX;

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
    test_remote_gate_stress(initial_mask)?;
    println!("sched-attr-test: END all available cases passed");
    Ok(())
}
