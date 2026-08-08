//! Userspace oracle for the eight native Linux clock IDs.
//!
//! This intentionally uses raw syscalls so libc policy cannot hide clock-ID,
//! layout, resolution, or errno mistakes in the kernel ABI.

use anemone_rs::{
    abi::{
        syscall::{SYS_CLOCK_GETRES, SYS_CLOCK_GETTIME, syscall},
        time::linux::{
            TimeSpec,
            clock::{
                CLOCK_BOOTTIME, CLOCK_MONOTONIC, CLOCK_MONOTONIC_COARSE, CLOCK_MONOTONIC_RAW,
                CLOCK_PROCESS_CPUTIME_ID, CLOCK_REALTIME, CLOCK_REALTIME_COARSE,
                CLOCK_THREAD_CPUTIME_ID,
            },
        },
    },
    os::linux::time,
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

fn clock_now(clock_id: i32) -> u64 {
    let value = time::clock_gettime(clock_id).unwrap();
    assert!(value.tv_sec >= 0);
    assert!((0..NANOS_PER_SEC as i64).contains(&value.tv_nsec));
    (value.tv_sec as u64)
        .checked_mul(NANOS_PER_SEC)
        .and_then(|seconds| seconds.checked_add(value.tv_nsec as u64))
        .unwrap()
}

fn clock_resolution(clock_id: i32) -> u64 {
    let value = time::clock_getres(clock_id).unwrap();
    assert!(value.tv_sec >= 0);
    (value.tv_sec as u64)
        .checked_mul(NANOS_PER_SEC)
        .and_then(|seconds| seconds.checked_add(value.tv_nsec as u64))
        .unwrap()
}

#[derive(Clone, Copy)]
pub(crate) struct BootBaseline {
    pub(crate) offset_ns: u64,
}

/// Capture and validate the calendar anchor before any test mutates it.
pub(crate) fn verify_boot_walltime() -> BootBaseline {
    let monotonic_before = clock_now(CLOCK_MONOTONIC);
    let realtime = clock_now(CLOCK_REALTIME);
    let monotonic_after = clock_now(CLOCK_MONOTONIC);
    let epoch_floor = 1_577_836_800_u64 * NANOS_PER_SEC; // 2020-01-01
    assert!(
        realtime >= epoch_floor,
        "RTC seed left realtime near the boot epoch"
    );
    let lower_offset = realtime
        .checked_sub(monotonic_after)
        .expect("realtime must be ahead of monotonic");
    let upper_offset = realtime
        .checked_sub(monotonic_before)
        .expect("realtime must be ahead of monotonic");
    assert!(lower_offset <= upper_offset);

    let coarse_before = clock_now(CLOCK_MONOTONIC_COARSE);
    let realtime_coarse = clock_now(CLOCK_REALTIME_COARSE);
    let coarse_after = clock_now(CLOCK_MONOTONIC_COARSE);
    let coarse_lower = realtime_coarse
        .checked_sub(coarse_after)
        .expect("coarse realtime must be ahead of coarse monotonic");
    let coarse_upper = realtime_coarse
        .checked_sub(coarse_before)
        .expect("coarse realtime must be ahead of coarse monotonic");
    assert!(lower_offset <= coarse_upper && coarse_lower <= upper_offset);
    println!(
        "boot-walltime: realtime_ns={realtime} monotonic_ns={monotonic_after} offset_ns={upper_offset} coarse_offset_ns={coarse_lower}"
    );
    BootBaseline {
        offset_ns: upper_offset,
    }
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
    time::nanosleep(duration).unwrap();
}

pub(crate) fn verify_native_clocks() {
    let mut resolutions = [0u64; CLOCKS.len()];

    for (index, (name, clock_id)) in CLOCKS.iter().copied().enumerate() {
        let first = clock_now(clock_id);
        let second = clock_now(clock_id);
        assert!(second >= first, "{name} regressed between ordered reads");

        let resolution = clock_resolution(clock_id);
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

    // Raw and boottime remain boot-relative; realtime now carries the boot
    // calendar offset and must not be compared directly with monotonic.
    let monotonic_before = clock_now(CLOCK_MONOTONIC);
    let raw = clock_now(CLOCK_MONOTONIC_RAW);
    let boottime = clock_now(CLOCK_BOOTTIME);
    let monotonic_after = clock_now(CLOCK_MONOTONIC);
    for (name, value) in [("raw", raw), ("boottime", boottime)] {
        assert!(
            (monotonic_before..=monotonic_after).contains(&value),
            "{name} did not use the Gate 1 monotonic derivation"
        );
    }

    let coarse_before = clock_now(CLOCK_MONOTONIC_COARSE);
    let realtime_coarse = clock_now(CLOCK_REALTIME_COARSE);
    let coarse_after = clock_now(CLOCK_MONOTONIC_COARSE);
    assert!(coarse_after >= coarse_before);
    assert!(
        realtime_coarse > coarse_after,
        "realtime-coarse lost the boot calendar offset"
    );

    // Busy work must advance task-owned CPU clocks, while blocked wall time must
    // not be charged as CPU consumption.
    let process_before = clock_now(CLOCK_PROCESS_CPUTIME_ID);
    let thread_before = clock_now(CLOCK_THREAD_CPUTIME_ID);
    for _ in 0..100_000 {
        core::hint::spin_loop();
    }
    let process_after = clock_now(CLOCK_PROCESS_CPUTIME_ID);
    let thread_after = clock_now(CLOCK_THREAD_CPUTIME_ID);
    assert!(process_after > process_before);
    assert!(thread_after > thread_before);

    let monotonic_before_sleep = clock_now(CLOCK_MONOTONIC);
    let process_before_sleep = clock_now(CLOCK_PROCESS_CPUTIME_ID);
    let thread_before_sleep = clock_now(CLOCK_THREAD_CPUTIME_ID);
    nanosleep(50_000_000);
    let process_after_sleep = clock_now(CLOCK_PROCESS_CPUTIME_ID);
    let thread_after_sleep = clock_now(CLOCK_THREAD_CPUTIME_ID);
    let monotonic_after_sleep = clock_now(CLOCK_MONOTONIC);
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
