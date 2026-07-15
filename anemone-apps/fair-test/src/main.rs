#![no_std]
#![no_main]

use core::{
    hint::black_box,
    mem::size_of,
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};

use anemone_rs::{
    abi::time::linux::TimeSpec,
    os::linux::{
        process::{
            MmapFlags, MmapProt, PriorityWhich, WStatus, WStatusRaw, WaitFor, WaitOptions, exit,
            fork, getpriority, mmap, munmap, sched_yield, setpriority, wait4,
        },
        time::{gettimeofday, nanosleep},
    },
    prelude::*,
};

const MAX_WORKERS: usize = 4;
const TEST_DURATION_US: u64 = 2_000_000;
const READY_TIMEOUT_US: u64 = 5_000_000;
const BURN_ITERS: usize = 2048;
const SLEEP_TICK_NS: i64 = 1_000_000;

#[derive(Clone, Copy)]
enum WorkerKind {
    Busy,
    Nice(i32),
    Yielding,
    Sleeper,
}

#[repr(C)]
struct SharedRun {
    ready: AtomicUsize,
    go: AtomicBool,
    end_us: AtomicU64,
    counters: [AtomicU64; MAX_WORKERS],
}

impl SharedRun {
    const fn new() -> Self {
        Self {
            ready: AtomicUsize::new(0),
            go: AtomicBool::new(false),
            end_us: AtomicU64::new(0),
            counters: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
        }
    }
}

fn now_us() -> Result<u64, Errno> {
    let now = gettimeofday()?;
    assert!(
        now.tv_sec >= 0 && (0..1_000_000).contains(&now.tv_usec),
        "fair-test: gettimeofday returned an invalid timestamp"
    );
    Ok(now.tv_sec as u64 * 1_000_000 + now.tv_usec as u64)
}

#[inline(never)]
fn burn_batch(mut value: u64) -> u64 {
    for _ in 0..BURN_ITERS {
        value = black_box(
            value
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407),
        );
    }
    value
}

fn sleep_tick() -> Result<(), Errno> {
    let duration = TimeSpec {
        tv_sec: 0,
        tv_nsec: SLEEP_TICK_NS,
    };
    loop {
        match nanosleep(duration) {
            Ok(()) => return Ok(()),
            Err(EINTR) => {},
            Err(errno) => return Err(errno),
        }
    }
}

fn wait_for_start(shared: &SharedRun) -> Result<u64, Errno> {
    shared.ready.fetch_add(1, Ordering::AcqRel);
    while !shared.go.load(Ordering::Acquire) {
        sched_yield()?;
    }
    Ok(shared.end_us.load(Ordering::Acquire))
}

fn configure_worker(kind: WorkerKind) -> Result<(), Errno> {
    let WorkerKind::Nice(nice) = kind else {
        return Ok(());
    };

    setpriority(PriorityWhich::Process, 0, nice)?;
    assert_eq!(
        getpriority(PriorityWhich::Process, 0)?,
        nice,
        "fair-test: setpriority result is not observable"
    );
    Ok(())
}

fn worker(shared: &SharedRun, index: usize, kind: WorkerKind) -> ! {
    configure_worker(kind).expect("fair-test: failed to configure worker priority");
    let end_us = wait_for_start(shared).expect("fair-test: worker start barrier failed");
    let mut batches = 0u64;
    let mut value = index as u64 + 1;

    while now_us().expect("fair-test: worker gettimeofday failed") < end_us {
        match kind {
            WorkerKind::Busy | WorkerKind::Nice(_) => {
                value = burn_batch(value);
            },
            WorkerKind::Yielding => {
                value = burn_batch(value);
                sched_yield().expect("fair-test: worker sched_yield failed");
            },
            WorkerKind::Sleeper => {
                sleep_tick().expect("fair-test: worker nanosleep failed");
                value = burn_batch(value);
            },
        }
        batches = batches.saturating_add(1);
    }

    black_box(value);
    shared.counters[index].store(batches, Ordering::Release);
    exit(0)
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
                assert_eq!(waited, pid, "fair-test: waited pid mismatch");
                match status.read() {
                    WStatus::Exited(0) => return Ok(()),
                    other => panic!("fair-test: worker {pid} exited unexpectedly: {other:?}"),
                }
            },
            Ok(None) => panic!("fair-test: wait4 returned None without WNOHANG"),
            Err(EINTR) => {},
            Err(errno) => return Err(errno),
        }
    }
}

fn run_workers(kinds: &[WorkerKind]) -> Result<Vec<u64>, Errno> {
    assert!(!kinds.is_empty() && kinds.len() <= MAX_WORKERS);
    let mapping = mmap(
        0,
        size_of::<SharedRun>(),
        MmapProt::PROT_READ | MmapProt::PROT_WRITE,
        MmapFlags::MAP_SHARED | MmapFlags::MAP_ANONYMOUS,
        None,
        None,
    )?;
    let shared_ptr = mapping.as_ptr().cast::<SharedRun>();
    unsafe { shared_ptr.write(SharedRun::new()) };
    let shared = unsafe { &*shared_ptr };

    let mut children = Vec::with_capacity(kinds.len());
    for (index, kind) in kinds.iter().copied().enumerate() {
        match fork()? {
            Some(pid) => children.push(pid),
            None => worker(shared, index, kind),
        }
    }

    let ready_deadline = now_us()?.saturating_add(READY_TIMEOUT_US);
    while shared.ready.load(Ordering::Acquire) != kinds.len() {
        assert!(
            now_us()? < ready_deadline,
            "fair-test: timed out waiting for workers to reach the start barrier"
        );
        sched_yield()?;
    }

    shared.end_us.store(
        now_us()?.saturating_add(TEST_DURATION_US),
        Ordering::Release,
    );
    shared.go.store(true, Ordering::Release);

    for child in children {
        wait_child_exit(child)?;
    }

    let counts = (0..kinds.len())
        .map(|index| shared.counters[index].load(Ordering::Acquire))
        .collect();
    munmap(mapping.as_ptr(), size_of::<SharedRun>())?;
    Ok(counts)
}

fn test_equal_progress() -> Result<(), Errno> {
    println!("fair-test: CASE equal-progress start");
    let counts = run_workers(&[
        WorkerKind::Busy,
        WorkerKind::Busy,
        WorkerKind::Busy,
        WorkerKind::Busy,
    ])?;
    let min = *counts.iter().min().unwrap();
    let max = *counts.iter().max().unwrap();
    assert!(min > 0, "fair-test: equal worker made no progress");
    assert!(
        max <= min.saturating_mul(2),
        "fair-test: equal worker spread exceeds 2x: {counts:?}"
    );
    println!("fair-test: CASE equal-progress ok counts={counts:?}");
    Ok(())
}

fn test_nice_direction() -> Result<(), Errno> {
    println!("fair-test: CASE nice-direction start");
    let counts = run_workers(&[WorkerKind::Busy, WorkerKind::Nice(5)])?;
    let normal = counts[0];
    let nice_five = counts[1];
    assert!(normal > 0 && nice_five > 0);
    assert!(
        normal.saturating_mul(2) >= nice_five.saturating_mul(3),
        "fair-test: nice 0 worker did not receive at least 1.5x nice 5 share: {counts:?}"
    );
    println!("fair-test: CASE nice-direction ok counts={counts:?}");
    Ok(())
}

fn test_bounded_yield() -> Result<(), Errno> {
    println!("fair-test: CASE bounded-yield start");
    let counts = run_workers(&[WorkerKind::Yielding, WorkerKind::Busy])?;
    let yielding = counts[0];
    let peer = counts[1];
    assert!(yielding > 0, "fair-test: yielding worker was starved");
    assert!(
        peer > yielding,
        "fair-test: yield did not favor the runnable peer"
    );
    println!("fair-test: CASE bounded-yield ok counts={counts:?}");
    Ok(())
}

fn test_sleep_wake_progress() -> Result<(), Errno> {
    println!("fair-test: CASE sleep-wake-progress start");
    let counts = run_workers(&[WorkerKind::Sleeper, WorkerKind::Busy])?;
    let sleeper = counts[0];
    let peer = counts[1];
    assert!(
        sleeper >= 10,
        "fair-test: sleep/wake worker made insufficient progress: {counts:?}"
    );
    assert!(peer > 0, "fair-test: CPU-bound wake peer was starved");
    println!("fair-test: CASE sleep-wake-progress ok counts={counts:?}");
    Ok(())
}

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    println!("fair-test: BEGIN");
    test_equal_progress()?;
    test_nice_direction()?;
    test_bounded_yield()?;
    test_sleep_wake_progress()?;
    println!("fair-test: END all cases passed");
    Ok(())
}
