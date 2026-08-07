//! Userspace oracle for the eight native Linux clock IDs.
//!
//! This intentionally uses raw syscalls so libc policy cannot hide clock-ID,
//! layout, resolution, or errno mistakes in the kernel ABI.

use anemone_rs::{
    abi::{
        syscall::{SYS_CLOCK_GETRES, SYS_CLOCK_GETTIME, SYS_NANOSLEEP, syscall},
        time::linux::{
            TimeSpec,
            clock::{
                CLOCK_BOOTTIME, CLOCK_MONOTONIC, CLOCK_MONOTONIC_COARSE, CLOCK_MONOTONIC_RAW,
                CLOCK_PROCESS_CPUTIME_ID, CLOCK_REALTIME, CLOCK_REALTIME_COARSE,
                CLOCK_THREAD_CPUTIME_ID,
            },
        },
    },
    prelude::*,
};

const NANOS_PER_SEC: u64 = 1_000_000_000;
const CLOCKS: [(&str, i32); 8] = [
    ("realtime", CLOCK_REALTIME),
    ("monotonic", CLOCK_MONOTONIC),
    ("process-cputime", CLOCK_PROCESS_CPUTIME_ID),
    ("thread-cputime", CLOCK_THREAD_CPUTIME_ID),
    ("monotonic-raw", CLOCK_MONOTONIC_RAW),
    ("realtime-coarse", CLOCK_REALTIME_COARSE),
    ("monotonic-coarse", CLOCK_MONOTONIC_COARSE),
    ("boottime", CLOCK_BOOTTIME),
];

fn clock_value(syscall_number: u64, clock_id: i32) -> Result<u64, Errno> {
    let mut value = TimeSpec::default();
    unsafe {
        syscall(
            syscall_number,
            clock_id as u64,
            (&mut value as *mut TimeSpec) as u64,
            0,
            0,
            0,
            0,
        )?;
    }

    assert!(
        value.tv_sec >= 0,
        "clock {clock_id} returned negative seconds"
    );
    assert!(
        (0..NANOS_PER_SEC as i64).contains(&value.tv_nsec),
        "clock {clock_id} returned invalid nanoseconds"
    );
    (value.tv_sec as u64)
        .checked_mul(NANOS_PER_SEC)
        .and_then(|seconds| seconds.checked_add(value.tv_nsec as u64))
        .ok_or(EOVERFLOW)
}

fn expect_invalid_clock(syscall_number: u64) {
    let mut value = TimeSpec::default();
    let result = unsafe {
        syscall(
            syscall_number,
            CLOCKS.len() as u64,
            (&mut value as *mut TimeSpec) as u64,
            0,
            0,
            0,
            0,
        )
    };
    assert_eq!(result, Err(EINVAL));
}

fn nanosleep(nanoseconds: i64) {
    let duration = TimeSpec {
        tv_sec: 0,
        tv_nsec: nanoseconds,
    };
    unsafe {
        syscall(
            SYS_NANOSLEEP,
            (&duration as *const TimeSpec) as u64,
            0,
            0,
            0,
            0,
            0,
        )
        .unwrap();
    }
}

pub(crate) fn verify_native_clocks() {
    let mut resolutions = [0u64; CLOCKS.len()];

    for (index, (name, clock_id)) in CLOCKS.iter().copied().enumerate() {
        let first = clock_value(SYS_CLOCK_GETTIME, clock_id).unwrap();
        let second = clock_value(SYS_CLOCK_GETTIME, clock_id).unwrap();
        assert!(second >= first, "{name} regressed between ordered reads");

        let resolution = clock_value(SYS_CLOCK_GETRES, clock_id).unwrap();
        assert!(resolution > 0, "{name} reported zero resolution");
        resolutions[index] = resolution;
        println!("clock-read: {name} time_ns={second} resolution_ns={resolution}");
    }

    // Ordinary and CPU clocks share source-counter precision in Gate 1; coarse
    // clocks form a separate resolution class based on BSP tick publication.
    for clock_id in [
        CLOCK_REALTIME,
        CLOCK_MONOTONIC,
        CLOCK_PROCESS_CPUTIME_ID,
        CLOCK_THREAD_CPUTIME_ID,
        CLOCK_MONOTONIC_RAW,
        CLOCK_BOOTTIME,
    ] {
        assert_eq!(
            resolutions[clock_id as usize],
            resolutions[CLOCK_MONOTONIC as usize]
        );
    }
    assert_eq!(
        resolutions[CLOCK_REALTIME_COARSE as usize],
        resolutions[CLOCK_MONOTONIC_COARSE as usize]
    );
    assert!(resolutions[CLOCK_MONOTONIC_COARSE as usize] >= resolutions[CLOCK_MONOTONIC as usize]);

    // With no RTC seed, correction, or suspend accounting, these projections
    // must land inside one bracketing monotonic read interval.
    let monotonic_before = clock_value(SYS_CLOCK_GETTIME, CLOCK_MONOTONIC).unwrap();
    let realtime = clock_value(SYS_CLOCK_GETTIME, CLOCK_REALTIME).unwrap();
    let raw = clock_value(SYS_CLOCK_GETTIME, CLOCK_MONOTONIC_RAW).unwrap();
    let boottime = clock_value(SYS_CLOCK_GETTIME, CLOCK_BOOTTIME).unwrap();
    let monotonic_after = clock_value(SYS_CLOCK_GETTIME, CLOCK_MONOTONIC).unwrap();
    for (name, value) in [("realtime", realtime), ("raw", raw), ("boottime", boottime)] {
        assert!(
            (monotonic_before..=monotonic_after).contains(&value),
            "{name} did not use the Gate 1 monotonic derivation"
        );
    }

    let coarse_before = clock_value(SYS_CLOCK_GETTIME, CLOCK_MONOTONIC_COARSE).unwrap();
    let realtime_coarse = clock_value(SYS_CLOCK_GETTIME, CLOCK_REALTIME_COARSE).unwrap();
    let coarse_after = clock_value(SYS_CLOCK_GETTIME, CLOCK_MONOTONIC_COARSE).unwrap();
    assert!(
        (coarse_before..=coarse_after).contains(&realtime_coarse),
        "realtime-coarse did not use the Gate 1 coarse monotonic snapshot"
    );

    // Busy work must advance task-owned CPU clocks, while blocked wall time must
    // not be charged as CPU consumption.
    let process_before = clock_value(SYS_CLOCK_GETTIME, CLOCK_PROCESS_CPUTIME_ID).unwrap();
    let thread_before = clock_value(SYS_CLOCK_GETTIME, CLOCK_THREAD_CPUTIME_ID).unwrap();
    for _ in 0..100_000 {
        core::hint::spin_loop();
    }
    let process_after = clock_value(SYS_CLOCK_GETTIME, CLOCK_PROCESS_CPUTIME_ID).unwrap();
    let thread_after = clock_value(SYS_CLOCK_GETTIME, CLOCK_THREAD_CPUTIME_ID).unwrap();
    assert!(process_after > process_before);
    assert!(thread_after > thread_before);

    let monotonic_before_sleep = clock_value(SYS_CLOCK_GETTIME, CLOCK_MONOTONIC).unwrap();
    let process_before_sleep = clock_value(SYS_CLOCK_GETTIME, CLOCK_PROCESS_CPUTIME_ID).unwrap();
    let thread_before_sleep = clock_value(SYS_CLOCK_GETTIME, CLOCK_THREAD_CPUTIME_ID).unwrap();
    nanosleep(50_000_000);
    let process_after_sleep = clock_value(SYS_CLOCK_GETTIME, CLOCK_PROCESS_CPUTIME_ID).unwrap();
    let thread_after_sleep = clock_value(SYS_CLOCK_GETTIME, CLOCK_THREAD_CPUTIME_ID).unwrap();
    let monotonic_after_sleep = clock_value(SYS_CLOCK_GETTIME, CLOCK_MONOTONIC).unwrap();
    let elapsed = monotonic_after_sleep - monotonic_before_sleep;
    assert!(elapsed >= 50_000_000);
    assert!(process_after_sleep - process_before_sleep < elapsed / 4);
    assert!(thread_after_sleep - thread_before_sleep < elapsed / 4);

    expect_invalid_clock(SYS_CLOCK_GETTIME);
    expect_invalid_clock(SYS_CLOCK_GETRES);
    assert_eq!(
        unsafe { syscall(SYS_CLOCK_GETRES, CLOCK_MONOTONIC as u64, 0, 0, 0, 0, 0) },
        Ok(0)
    );
    println!("clock-read: all native clock checks passed");
}
