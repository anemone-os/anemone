#![no_std]
#![no_main]

use anemone_rs::{
    abi::system::native::perf::{PERF_ELAPSED_SAMPLE_COUNT_INDEX, PERF_ELAPSED_SUM_INDEX},
    env::args,
    os::{
        anemone::debug::perf::{
            PerfCatalog, PerfMetricKind, PerfMetricUnit, PerfSnapshot, get_enabled, query,
            set_enabled, snapshot,
        },
        linux::process::{
            WStatus, WStatusRaw, WaitFor, WaitOptions, execve, exit, fork, getrusage_children,
            wait4,
        },
    },
    prelude::*,
};

fn kind_name(kind: PerfMetricKind) -> &'static str {
    match kind {
        PerfMetricKind::Counter => "counter",
        PerfMetricKind::Histogram => "histogram",
        PerfMetricKind::Elapsed => "elapsed",
    }
}

fn unit_name(unit: PerfMetricUnit) -> &'static str {
    match unit {
        PerfMetricUnit::Events => "events",
        PerfMetricUnit::MonotonicTicks => "monotonic-ticks",
    }
}

fn print_catalog(catalog: &PerfCatalog) {
    println!(
        "clock=monotonic-raw frequency-hz={} metrics={} values={} histogram-buckets={}",
        catalog.clock_frequency_hz,
        catalog.metrics.len(),
        catalog.value_count,
        catalog.histogram_bucket_count,
    );
    for metric in &catalog.metrics {
        println!(
            "{} id={} kind={} unit={} values={}@{}",
            metric.name,
            metric.id,
            kind_name(metric.kind),
            unit_name(metric.unit),
            metric.value_count,
            metric.value_offset,
        );
    }
}

fn print_snapshot(catalog: &PerfCatalog, image: &PerfSnapshot) -> Result<(), Errno> {
    if image.values.len() != catalog.value_count {
        return Err(EINVAL);
    }
    println!(
        "window={}..={} enabled={}",
        image.begin_ticks, image.end_ticks, image.enabled
    );
    for metric in &catalog.metrics {
        let values = &image.values[metric.value_offset..metric.value_offset + metric.value_count];
        match metric.kind {
            PerfMetricKind::Counter => println!("{}={}", metric.name, values[0]),
            PerfMetricKind::Histogram => {
                print!(
                    "{} sum_ticks={} buckets=[",
                    metric.name, values[catalog.histogram_bucket_count],
                );
                for (index, value) in values[..catalog.histogram_bucket_count].iter().enumerate() {
                    if *value != 0 {
                        print!("{}:{} ", index, value);
                    }
                }
                println!("]");
            },
            PerfMetricKind::Elapsed => println!(
                "{} samples={} sum_ticks={}",
                metric.name,
                values[PERF_ELAPSED_SAMPLE_COUNT_INDEX],
                values[PERF_ELAPSED_SUM_INDEX],
            ),
        }
    }
    Ok(())
}

fn print_delta(
    catalog: &PerfCatalog,
    before: &PerfSnapshot,
    after: &PerfSnapshot,
) -> Result<(), Errno> {
    let delta = after.wrapping_delta_from(before)?;
    println!(
        "delta window_ticks={} clock_frequency_hz={}",
        after.begin_ticks.wrapping_sub(before.end_ticks),
        catalog.clock_frequency_hz,
    );
    for metric in &catalog.metrics {
        let values = &delta[metric.value_offset..metric.value_offset + metric.value_count];
        match metric.kind {
            PerfMetricKind::Counter => {
                if values[0] != 0 {
                    println!("delta {}={}", metric.name, values[0]);
                }
            },
            PerfMetricKind::Histogram => {
                let buckets = &values[..catalog.histogram_bucket_count];
                let samples = buckets
                    .iter()
                    .fold(0u64, |sum, value| sum.wrapping_add(*value));
                if samples == 0 {
                    continue;
                }
                print!(
                    "delta {} samples={} sum_ticks={} buckets=[",
                    metric.name, samples, values[catalog.histogram_bucket_count],
                );
                for (index, value) in buckets.iter().enumerate() {
                    if *value != 0 {
                        print!("{}:{} ", index, value);
                    }
                }
                println!("]");
            },
            PerfMetricKind::Elapsed => {
                if values[PERF_ELAPSED_SAMPLE_COUNT_INDEX] != 0 {
                    println!(
                        "delta {} samples={} sum_ticks={}",
                        metric.name,
                        values[PERF_ELAPSED_SAMPLE_COUNT_INDEX],
                        values[PERF_ELAPSED_SUM_INDEX],
                    );
                }
            },
        }
    }
    Ok(())
}

fn wait_child(child: u32) -> Result<(), Errno> {
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
        WStatus::Exited(0) => Ok(()),
        _ => Err(EIO),
    }
}

fn basis_points(part: u64, whole: u64) -> u64 {
    if whole == 0 {
        return 0;
    }
    ((u128::from(part) * 10_000) / u128::from(whole)).min(u128::from(u64::MAX)) as u64
}

fn timeval_micros(time: anemone_rs::abi::time::linux::TimeVal) -> Result<u64, Errno> {
    let seconds = u64::try_from(time.tv_sec).map_err(|_| EIO)?;
    let micros = u64::try_from(time.tv_usec).map_err(|_| EIO)?;
    if micros >= 1_000_000 {
        return Err(EIO);
    }
    seconds
        .checked_mul(1_000_000)
        .and_then(|value| value.checked_add(micros))
        .ok_or(EOVERFLOW)
}

fn print_syscall_delta(
    catalog: &PerfCatalog,
    before: &PerfSnapshot,
    after: &PerfSnapshot,
    usage: anemone_rs::abi::process::linux::resource::RUsage,
) -> Result<(), Errno> {
    let delta = after.wrapping_delta_from(before)?;
    let window_ticks = after.begin_ticks.wrapping_sub(before.end_ticks);
    let mut rows = Vec::new();
    for (index, metric) in catalog.metrics.iter().enumerate() {
        let Some(syscall) = metric
            .name
            .strip_prefix("syscall.")
            .and_then(|name| name.strip_suffix(".kernel_cpu"))
        else {
            continue;
        };
        if metric.kind != PerfMetricKind::Elapsed {
            return Err(EIO);
        }
        let residence = catalog
            .metrics
            .iter()
            .find(|other| {
                other
                    .name
                    .strip_prefix("syscall.")
                    .and_then(|name| name.strip_suffix(".elapsed"))
                    == Some(syscall)
            })
            .ok_or(EIO)?;
        if residence.kind != PerfMetricKind::Elapsed {
            return Err(EIO);
        }
        let cpu_values = &delta[metric.value_offset..metric.value_offset + metric.value_count];
        let residence_values =
            &delta[residence.value_offset..residence.value_offset + residence.value_count];
        let calls = cpu_values[PERF_ELAPSED_SAMPLE_COUNT_INDEX];
        if calls != 0 {
            rows.push((
                index,
                calls,
                cpu_values[PERF_ELAPSED_SUM_INDEX],
                residence_values[PERF_ELAPSED_SAMPLE_COUNT_INDEX],
                residence_values[PERF_ELAPSED_SUM_INDEX],
            ));
        }
    }
    rows.sort_unstable_by(|left, right| {
        right.2.cmp(&left.2).then_with(|| {
            catalog.metrics[left.0]
                .name
                .cmp(&catalog.metrics[right.0].name)
        })
    });

    println!(
        "syscall-profile scope=system-wide semantics=completed-returning-wrapper-invocations window_ticks={} clock_frequency_hz={}",
        window_ticks, catalog.clock_frequency_hz,
    );
    let mut total_kernel_cpu_ticks = 0u64;
    for (index, calls, kernel_cpu_ticks, residence_calls, residence_ticks) in rows {
        let name = catalog.metrics[index]
            .name
            .strip_prefix("syscall.")
            .and_then(|name| name.strip_suffix(".kernel_cpu"))
            .ok_or(EIO)?;
        println!(
            "syscall {} calls={} residence_calls={} kernel_cpu_ticks={} residence_ticks={} mean_kernel_cpu_ticks={} kernel_cpu_wall_basis_points={}",
            name,
            calls,
            residence_calls,
            kernel_cpu_ticks,
            residence_ticks,
            kernel_cpu_ticks / calls,
            basis_points(kernel_cpu_ticks, window_ticks),
        );
        total_kernel_cpu_ticks = total_kernel_cpu_ticks.wrapping_add(kernel_cpu_ticks);
    }
    let child_user_us = timeval_micros(usage.ru_utime)?;
    let child_kernel_us = timeval_micros(usage.ru_stime)?;
    println!(
        "reaped-child-cpu user_us={} kernel_us={} total_us={}",
        child_user_us,
        child_kernel_us,
        child_user_us
            .checked_add(child_kernel_us)
            .ok_or(EOVERFLOW)?,
    );
    println!(
        "syscall-cpu-total sum_ticks={} wall_basis_points={} caveat=system-wide-not-conserved-against-child-cpu",
        total_kernel_cpu_ticks,
        basis_points(total_kernel_cpu_ticks, window_ticks),
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum Report {
    All,
    Syscalls,
}

fn run_command(path: &'static str, argv: &[&'static str], report: Report) -> Result<(), Errno> {
    let catalog = query()?;
    let old = set_enabled(true)?;
    let operation = (|| {
        let before = snapshot()?;
        match fork()? {
            Some(child) => wait_child(child)?,
            None => {
                if execve(path, argv, &[]).is_err() {
                    exit(127);
                }
                unreachable!();
            },
        }
        let after = snapshot()?;
        match report {
            Report::All => print_delta(&catalog, &before, &after),
            Report::Syscalls => {
                // Child accounting is queried after the perf window so this
                // control syscall cannot contaminate its own report.
                print_syscall_delta(&catalog, &before, &after, getrusage_children()?)
            },
        }
    })();
    let restore = set_enabled(old);
    restore?;
    operation
}

fn usage() {
    println!(
        "usage: perfctl list|status|enable|disable|snapshot|run <program> [args...]|syscalls run <program> [args...]"
    );
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    let mut argv = args();
    let _program = argv.next();
    match argv.next() {
        Some("list") => print_catalog(&query()?),
        Some("status") => println!(
            "{}",
            if get_enabled()? {
                "enabled"
            } else {
                "disabled"
            }
        ),
        Some("enable") => println!("previous={}", set_enabled(true)?),
        Some("disable") => println!("previous={}", set_enabled(false)?),
        Some("snapshot") => {
            let catalog = query()?;
            print_snapshot(&catalog, &snapshot()?)?;
        },
        Some("run") => {
            let Some(path) = argv.next() else {
                usage();
                return Err(EINVAL);
            };
            let mut command_argv = vec![path];
            command_argv.extend(argv);
            run_command(path, &command_argv, Report::All)?;
        },
        Some("syscalls") => {
            if argv.next() != Some("run") {
                usage();
                return Err(EINVAL);
            }
            let Some(path) = argv.next() else {
                usage();
                return Err(EINVAL);
            };
            let mut command_argv = vec![path];
            command_argv.extend(argv);
            run_command(path, &command_argv, Report::Syscalls)?;
        },
        _ => {
            usage();
            return Err(EINVAL);
        },
    }
    Ok(())
}
