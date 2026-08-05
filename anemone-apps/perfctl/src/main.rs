#![no_std]
#![no_main]

use anemone_rs::{
    env::args,
    os::{
        anemone::debug::perf::{
            PerfCatalog, PerfMetricKind, PerfMetricUnit, PerfSnapshot, get_enabled, query,
            set_enabled, snapshot,
        },
        linux::process::{WStatus, WStatusRaw, WaitFor, WaitOptions, execve, exit, fork, wait4},
    },
    prelude::*,
};

fn kind_name(kind: PerfMetricKind) -> &'static str {
    match kind {
        PerfMetricKind::Counter => "counter",
        PerfMetricKind::Histogram => "histogram",
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
        if metric.kind == PerfMetricKind::Counter {
            println!("{}={}", metric.name, values[0]);
        } else {
            print!("{}=[", metric.name);
            for (index, value) in values.iter().enumerate() {
                if *value != 0 {
                    print!("{}:{} ", index, value);
                }
            }
            println!("]");
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
    for metric in &catalog.metrics {
        let values = &delta[metric.value_offset..metric.value_offset + metric.value_count];
        if metric.kind == PerfMetricKind::Counter {
            println!("delta {}={}", metric.name, values[0]);
        } else {
            let samples = values
                .iter()
                .fold(0u64, |sum, value| sum.wrapping_add(*value));
            println!("delta {} samples={}", metric.name, samples);
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

fn run_command(path: &'static str, argv: &[&'static str]) -> Result<(), Errno> {
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
        print_delta(&catalog, &before, &after)
    })();
    let restore = set_enabled(old);
    restore?;
    operation
}

fn usage() {
    println!("usage: perfctl list|status|enable|disable|snapshot|run <program> [args...]");
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
            run_command(path, &command_argv)?;
        },
        _ => {
            usage();
            return Err(EINVAL);
        },
    }
    Ok(())
}
