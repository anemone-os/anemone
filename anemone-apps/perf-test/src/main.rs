#![no_std]
#![no_main]

use core::mem::size_of;

use anemone_rs::{
    abi::{
        process::linux::sched::{CPU_SETSIZE, CpuSet},
        syscall::{SYS_PERF_OBSERVE, linux::*, syscall},
        system::native::{perf::*, power::SHUTDOWN_MAGIC},
    },
    os::{
        anemone::{
            debug::{
                get_log_levels,
                perf::{
                    PerfCatalog, PerfMetricDescriptor, PerfMetricKind, PerfMetricUnit, get_enabled,
                    query, set_enabled, snapshot,
                },
            },
            power::shutdown,
        },
        linux::{
            fs::STDOUT_FILENO,
            process::{WStatus, WStatusRaw, WaitFor, WaitOptions, execve, exit, fork, wait4},
            tty::{SetTermiosWhen, tcgetattr, tcsetattr},
        },
    },
    prelude::*,
    process::process_id,
};

// One stage's fork/wait/affinity path produces bounded local printk traffic.
// Keep the target workload far above that noise so a missing remote CPU read
// cannot satisfy the aggregate lower bound with setup records alone.
const PILOT_CALLS: usize = 4096;

fn raw_perf(op: u64, arg: u64, buf: u64, len: usize, flags: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_PERF_OBSERVE, op, arg, buf, len as u64, flags, 0) }
}

fn setuid(uid: u32) -> Result<(), Errno> {
    unsafe { syscall(SYS_SETUID, uid as u64, 0, 0, 0, 0, 0) }.map(|_| ())
}

fn sched_setaffinity(mask: &CpuSet) -> Result<(), Errno> {
    unsafe {
        syscall(
            SYS_SCHED_SETAFFINITY,
            0,
            size_of::<CpuSet>() as u64,
            mask as *const CpuSet as u64,
            0,
            0,
            0,
        )
    }
    .map(|_| ())
}

fn sched_getaffinity() -> Result<CpuSet, Errno> {
    let mut mask = CpuSet::empty();
    let copied = unsafe {
        syscall(
            SYS_SCHED_GETAFFINITY,
            0,
            size_of::<CpuSet>() as u64,
            &mut mask as *mut CpuSet as u64,
            0,
            0,
            0,
        )
    }?;
    if copied as usize != size_of::<usize>() {
        return Err(EIO);
    }
    Ok(mask)
}

fn singleton(cpu: usize) -> CpuSet {
    let mut mask = CpuSet::empty();
    mask.set(cpu);
    mask
}

#[track_caller]
fn expect_errno<T>(result: Result<T, Errno>, expected: Errno, what: &str) -> Result<(), Errno> {
    match result {
        Err(errno) if errno == expected => Ok(()),
        Ok(_) => {
            println!("perf-test: {what}: expected errno {expected}, got success");
            Err(EIO)
        },
        Err(errno) => {
            println!("perf-test: {what}: expected errno {expected}, got {errno}");
            Err(EIO)
        },
    }
}

fn wait_child_exit(child: u32) -> Result<i8, Errno> {
    let mut status = WStatusRaw::EMPTY;
    if wait4(
        WaitFor::ChildWithTgid(child),
        Some(&mut status),
        WaitOptions::empty(),
    )? != Some(child)
    {
        return Err(ECHILD);
    }
    match status.read() {
        WStatus::Exited(code) => Ok(code),
        _ => Err(EIO),
    }
}

fn wait_child(child: u32) -> Result<(), Errno> {
    if wait_child_exit(child)? == 0 {
        Ok(())
    } else {
        Err(EIO)
    }
}

fn metric<'a>(catalog: &'a PerfCatalog, name: &str) -> Result<&'a PerfMetricDescriptor, Errno> {
    catalog
        .metrics
        .iter()
        .find(|metric| metric.name == name)
        .ok_or(EINVAL)
}

fn validate_catalog(catalog: &PerfCatalog) -> Result<(), Errno> {
    if catalog.clock_frequency_hz == 0
        || catalog.histogram_bucket_count != PERF_HISTOGRAM_BUCKET_COUNT
    {
        return Err(EINVAL);
    }

    let records = metric(catalog, "debug.printk.records")?;
    if records.kind != PerfMetricKind::Counter
        || records.unit != PerfMetricUnit::Events
        || records.value_count != 1
    {
        return Err(EINVAL);
    }

    let latency = metric(catalog, "debug.printk.record_latency")?;
    if latency.kind != PerfMetricKind::Histogram
        || latency.unit != PerfMetricUnit::MonotonicTicks
        || latency.value_count != PERF_HISTOGRAM_BUCKET_COUNT
    {
        return Err(EINVAL);
    }
    Ok(())
}

fn validate_raw_abi(catalog: &PerfCatalog) -> Result<(), Errno> {
    let catalog_len = raw_perf(PERF_OBSERVE_QUERY, 0, 0, 0, 0)? as usize;
    let snapshot_len = PERF_SNAPSHOT_HEADER_SIZE + catalog.value_count * size_of::<u64>();
    expect_errno(raw_perf(u64::MAX, 0, 0, 0, 0), EINVAL, "unknown operation")?;
    expect_errno(
        raw_perf(PERF_OBSERVE_QUERY, 1, 0, 0, 0),
        EINVAL,
        "query argument",
    )?;
    expect_errno(
        raw_perf(PERF_OBSERVE_QUERY, 0, 0, 0, 1),
        EINVAL,
        "unknown flags",
    )?;
    expect_errno(
        raw_perf(PERF_OBSERVE_QUERY, 0, 1, catalog_len - 1, 0),
        ENOSPC,
        "query short buffer",
    )?;
    expect_errno(
        raw_perf(PERF_OBSERVE_QUERY, 0, u64::MAX, catalog_len, 0),
        EFAULT,
        "query bad address",
    )?;
    expect_errno(
        raw_perf(PERF_OBSERVE_SNAPSHOT, 0, 1, snapshot_len - 1, 0),
        ENOSPC,
        "snapshot short buffer",
    )?;
    expect_errno(
        raw_perf(PERF_OBSERVE_SNAPSHOT, 0, u64::MAX, snapshot_len, 0),
        EFAULT,
        "snapshot bad address",
    )?;
    let before = get_enabled()?;
    expect_errno(
        raw_perf(PERF_OBSERVE_SET_ENABLED, 2, 0, 0, 0),
        EINVAL,
        "invalid set value",
    )?;
    if get_enabled()? != before {
        let _ = set_enabled(before)?;
        return Err(EIO);
    }
    Ok(())
}

fn unprivileged_child(snapshot_len: usize) -> ! {
    let passed = setuid(1000).is_ok()
        && raw_perf(PERF_OBSERVE_QUERY, 0, 0, 0, 0).is_ok()
        && raw_perf(PERF_OBSERVE_GET_ENABLED, 0, 0, 0, 0).is_ok()
        && raw_perf(PERF_OBSERVE_SET_ENABLED, 1, 0, 0, 0) == Err(EPERM)
        && raw_perf(PERF_OBSERVE_SNAPSHOT, 0, 0, 0, 0) == Err(EPERM)
        && raw_perf(PERF_OBSERVE_SNAPSHOT, 0, 1, snapshot_len - 1, 0) == Err(ENOSPC);
    exit(if passed { 0 } else { 1 })
}

fn validate_permissions(catalog: &PerfCatalog) -> Result<(), Errno> {
    let snapshot_len = PERF_SNAPSHOT_HEADER_SIZE + catalog.value_count * size_of::<u64>();
    match fork()? {
        Some(child) => wait_child(child),
        None => unprivileged_child(snapshot_len),
    }
}

fn perfctl_child(argv: &'static [&'static str]) -> ! {
    if execve("/bin/perfctl", argv, &[]).is_err() {
        exit(127);
    }
    unreachable!()
}

fn run_perfctl(argv: &'static [&'static str]) -> Result<(), Errno> {
    match fork()? {
        Some(child) => wait_child(child),
        None => perfctl_child(argv),
    }
}

fn validate_perfctl_smoke() -> Result<(), Errno> {
    run_perfctl(&["perfctl", "list"])?;
    run_perfctl(&["perfctl", "status"])?;
    run_perfctl(&["perfctl", "snapshot"])?;
    run_perfctl(&["perfctl", "run", "/bin/perfctl", "status"])?;
    if get_enabled()? {
        let _ = set_enabled(false)?;
        return Err(EIO);
    }
    Ok(())
}

fn remote_pilot_child(cpu: usize) -> ! {
    match sched_setaffinity(&singleton(cpu)) {
        Ok(()) => {},
        Err(EINVAL) => exit(2),
        Err(_) => exit(3),
    }
    for _ in 0..PILOT_CALLS {
        if get_log_levels().is_err() {
            exit(4);
        }
    }
    exit(0)
}

fn run_remote_pilot(cpu: usize, cpu_count: usize) -> Result<(), Errno> {
    // Tasks have a fixed scheduler owner in this kernel. A child can narrow its
    // inherited mask to its own owner, while a child placed on another owner
    // reports EINVAL. The bounded retries follow the round-robin fork placement
    // and avoid pretending that sched_setaffinity migrates the reader task.
    for _ in 0..cpu_count * 2 {
        match fork()? {
            Some(child) => match wait_child_exit(child)? {
                0 => return Ok(()),
                2 => {},
                _ => return Err(EIO),
            },
            None => remote_pilot_child(cpu),
        }
    }
    Err(EIO)
}

fn validate_printk_pilot(catalog: &PerfCatalog) -> Result<(), Errno> {
    let initial_affinity = sched_getaffinity()?;
    let mut cpus = Vec::new();
    for cpu in 0..CPU_SETSIZE {
        if initial_affinity.contains(cpu) {
            cpus.push(cpu);
            if cpus.len() == 2 {
                break;
            }
        }
    }
    if cpus.is_empty() {
        return Err(EIO);
    }
    let reader_cpu = cpus[0];
    let records = metric(catalog, "debug.printk.records")?;
    let latency = metric(catalog, "debug.printk.record_latency")?;
    for (stage, cpu) in cpus.iter().enumerate() {
        println!("perf-test: pilot stage={stage} cpu={cpu} snapshot-before");
        let before = snapshot()?;
        if before.values.len() != catalog.value_count {
            return Err(EIO);
        }
        println!("perf-test: pilot stage={stage} cpu={cpu} workload");
        if stage == 0 {
            for _ in 0..PILOT_CALLS {
                let _ = get_log_levels()?;
            }
        } else {
            run_remote_pilot(*cpu, cpus.len())?;
        }
        println!("perf-test: pilot stage={stage} cpu={cpu} snapshot-after");
        let after = snapshot()?;
        if after.values.len() != catalog.value_count {
            return Err(EIO);
        }
        let delta = after.wrapping_delta_from(&before)?;
        let record_delta = delta[records.value_offset];
        let latency_delta = delta[latency.value_offset..latency.value_offset + latency.value_count]
            .iter()
            .fold(0u64, |sum, value| sum.wrapping_add(*value));
        if record_delta < PILOT_CALLS as u64 || latency_delta < PILOT_CALLS as u64 {
            println!(
                "perf-test: pilot stage={} cpu={} insufficient samples: records={} latency-samples={} required={}",
                stage, cpu, record_delta, latency_delta, PILOT_CALLS
            );
            return Err(EIO);
        }
        println!(
            "perf-test: pilot stage={} cpu={} reader={} records={} latency-samples={}",
            stage, cpu, reader_cpu, record_delta, latency_delta
        );
    }
    Ok(())
}

fn run_feature_on() -> Result<(), Errno> {
    if get_enabled()? {
        return Err(EIO);
    }
    let catalog = query()?;
    validate_catalog(&catalog)?;
    validate_raw_abi(&catalog)?;
    validate_permissions(&catalog)?;
    validate_perfctl_smoke()?;

    let old = set_enabled(true)?;
    if old {
        return Err(EIO);
    }
    let operation = (|| {
        if !get_enabled()? {
            return Err(EIO);
        }
        let image = snapshot()?;
        if !image.enabled || image.values.len() != catalog.value_count {
            return Err(EIO);
        }
        validate_printk_pilot(&catalog)
    })();
    let restored_from = set_enabled(old)?;
    if !restored_from {
        return Err(EIO);
    }
    operation?;
    if get_enabled()? {
        return Err(EIO);
    }
    Ok(())
}

fn run() -> Result<(), Errno> {
    match raw_perf(PERF_OBSERVE_QUERY, 0, 0, 0, 0) {
        Err(ENOSYS) => {
            println!("perf-test: feature-off ENOSYS ok");
            Ok(())
        },
        Ok(_) => run_feature_on(),
        Err(errno) => Err(errno),
    }
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    println!("perf-test: CASE native-abi-catalog-snapshot-pilot start");
    run()?;
    println!("perf-test: CASE native-abi-catalog-snapshot-pilot ok");

    if process_id() == 1 {
        let termios = tcgetattr(STDOUT_FILENO as _)?;
        tcsetattr(STDOUT_FILENO as _, SetTermiosWhen::Drain, &termios)?;
        shutdown(SHUTDOWN_MAGIC)?;
        unreachable!("perf-test: shutdown returned unexpectedly");
    }
    Ok(())
}
