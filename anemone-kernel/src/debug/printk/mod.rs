//! Kernel logging and printk-owned record presentation.

mod policy;
mod presentation;
mod record;
mod ring;

pub(crate) use policy::{LogPolicy, snapshot_policy};
pub(crate) use record::{LogCallsite, LogLevel};

use core::fmt::{self, Arguments, Write};

use crate::{
    prelude::*,
    utils::writer::{BufferWriter, OverflowBehavior},
};
use policy::replace_policy;
use presentation::{output_record, write_record};
use record::{BootTimestamp, LOG_RECORD_SIZE, LogRecord, LogRecordFlags};
use ring::KernelLog;

static KERNEL_LOG: KernelLog = KernelLog::new();

declare_perf_metrics! {
    counter PRINTK_RECORDS {
        name: "debug.printk.records",
        unit: Events,
    }
    histogram PRINTK_RECORD_LATENCY {
        name: "debug.printk.record_latency",
        unit: MonotonicTicks,
    }
}

pub(crate) fn validate_policy(levels: u64) -> Result<LogPolicy, SysError> {
    LogPolicy::from_packed(levels)
}

pub(crate) fn set_policy(policy: LogPolicy) -> LogPolicy {
    replace_policy(policy)
}

/// Replay the current best-effort ring view through a destination capability.
/// Printk retains policy, iteration and record-presentation ownership; console
/// never receives the ring or its private record representation.
pub(crate) fn replay_to(writer: &mut dyn Write) -> fmt::Result {
    let policy = snapshot_policy();
    for (sequence, record) in KERNEL_LOG.iter_weak() {
        if policy.prints(record.callsite.level) {
            write_record(writer, sequence, &record)?;
        }
    }
    Ok(())
}

/// Store one structured record, then optionally present that exact record.
/// `policy` is the macro's single pre-format snapshot.
pub(crate) fn __klog(
    callsite: &'static LogCallsite,
    policy: LogPolicy,
    msg: Arguments<'_>,
    noprint: bool,
) {
    assert!(policy.records(callsite.level));
    let timer = perf_timer!(PRINTK_RECORD_LATENCY);

    let timestamp = try_monotonic_uptime()
        .map(BootTimestamp::Monotonic)
        .unwrap_or(BootTimestamp::Unavailable);
    let record = LogRecord::from_args(callsite, timestamp, msg);
    let sequence = KERNEL_LOG.append(record);
    perf_counter_inc!(PRINTK_RECORDS);
    timer.finish();

    if !noprint && policy.prints(callsite.level) {
        output_record(sequence, &record);
    }
}

/// Raw level-less output remains a console-only fragment and does not acquire
/// structured record metadata.
pub(crate) fn __klog_raw(msg: Arguments<'_>, noprint: bool) {
    if noprint {
        return;
    }

    let mut buffer = [0; LOG_RECORD_SIZE];
    let mut writer = BufferWriter::<{ OverflowBehavior::TRUNCATE }>::new(&mut buffer);
    let _ = writer.write_fmt(msg);
    let len = writer.pos();
    drop(writer);
    let rendered = core::str::from_utf8(&buffer[..len])
        .expect("raw printk writer must preserve UTF-8 boundaries");
    device::console::output(rendered);
}

#[doc(hidden)]
#[macro_export]
macro_rules! __kprint_leveled {
    ($noprint:expr, $level:ident, $($arg:tt)*) => {{
        let level = $crate::debug::printk::LogLevel::$level;
        // Both gates precede `format_args!`: disabled callsites do not evaluate
        // formatting arguments or touch the record/console paths.
        if level.compile_enabled() {
            let policy = $crate::debug::printk::snapshot_policy();
            if policy.records(level) {
                static CALLSITE: $crate::debug::printk::LogCallsite =
                    $crate::debug::printk::LogCallsite {
                        level: $crate::debug::printk::LogLevel::$level,
                        module_path: module_path!(),
                        file: file!(),
                        line: line!(),
                    };
                $crate::debug::printk::__klog(
                    &CALLSITE,
                    policy,
                    format_args!($($arg)*),
                    $noprint,
                );
            }
        }
    }};
}

#[macro_export]
macro_rules! kprint {
    (noprint, $level:ident, $($arg:tt)*) => {
        $crate::__kprint_leveled!(true, $level, $($arg)*);
    };
    ($level:ident, $($arg:tt)*) => {
        $crate::__kprint_leveled!(false, $level, $($arg)*);
    };
    (noprint, $($arg:tt)*) => {
        $crate::debug::printk::__klog_raw(format_args!($($arg)*), true);
    };
    ($($arg:tt)*) => {
        $crate::debug::printk::__klog_raw(format_args!($($arg)*), false);
    };
}

#[macro_export]
macro_rules! kprintln {
    () => {
        $crate::kprint!("\n");
    };
    (noprint) => {
        $crate::kprint!(noprint, "\n");
    };
    (noprint, $level:ident, $($arg:tt)*) => {
        $crate::kprint!(noprint, $level, "{}\n", format_args!($($arg)*));
    };
    ($level:ident, $($arg:tt)*) => {
        $crate::kprint!($level, "{}\n", format_args!($($arg)*));
    };
    (noprint, $($arg:tt)*) => {
        $crate::kprint!(noprint, "{}\n", format_args!($($arg)*));
    };
    ($($arg:tt)*) => {
        $crate::kprint!("{}\n", format_args!($($arg)*));
    };
}

macro_rules! gen_printk_macros {
    ($dollar:tt, $($name:ident, $level:ident)*) => {
        paste::paste! {
            $(
                #[macro_export]
                macro_rules! [<k $name>] {
                    (noprint, $dollar($args:tt)*) => {
                        $crate::kprint!(noprint, $level, $dollar($args)*);
                    };
                    ($dollar($args:tt)*) => {
                        $crate::kprint!($level, $dollar($args)*);
                    }
                }

                #[macro_export]
                macro_rules! [<k $name ln>] {
                    () => {
                        $crate::kprint!($level, "\n");
                    };
                    (noprint) => {
                        $crate::kprint!(noprint, $level, "\n");
                    };
                    (noprint, $dollar($args:tt)*) => {
                        $crate::kprint!(noprint, $level, "{}\n", format_args!($dollar($args)*));
                    };
                    ($dollar($args:tt)*) => {
                        $crate::kprint!($level, "{}\n", format_args!($dollar($args)*));
                    }
                }
            )*
        }
    };
}

gen_printk_macros!(
    $,
    emerg, Emerg
    alert, Alert
    crit, Crit
    err, Err
    warning, Warning
    notice, Notice
    info, Info
    debug, Debug
);

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    static EVALUATED: AtomicUsize = AtomicUsize::new(0);

    fn evaluated_argument() -> usize {
        EVALUATED.fetch_add(1, Ordering::Relaxed)
    }

    #[kunit]
    fn compile_disabled_macro_does_not_evaluate_arguments() {
        if RECORD_LOG_LEVEL >= LogLevel::Debug as u8 {
            return;
        }

        EVALUATED.store(0, Ordering::Relaxed);
        kdebugln!(noprint, "{}", evaluated_argument());
        assert_eq!(EVALUATED.load(Ordering::Relaxed), 0);
    }

    #[kunit]
    fn macro_callsite_payload_and_single_policy_snapshot() {
        if RECORD_LOG_LEVEL < LogLevel::Debug as u8 {
            return;
        }

        let initial = snapshot_policy();
        let debug_record_only = validate_policy(LogLevel::Debug as u64).unwrap();
        set_policy(debug_record_only);
        let expected_line = line!() + 1;
        kdebugln!(noprint, "callsite-check");
        let (_, record) = KERNEL_LOG.iter_weak().last().unwrap();
        assert_eq!(record.callsite.level, LogLevel::Debug);
        assert_eq!(record.callsite.module_path, module_path!());
        assert_eq!(record.callsite.file, file!());
        assert_eq!(record.callsite.line, expected_line);
        assert_eq!(record.message(), "callsite-check\n");
        assert!(!record.message().contains("DEBUG"));
        assert!(!record.message().contains("\x1b["));

        let captured = snapshot_policy();
        set_policy(validate_policy(0).unwrap());
        static CAPTURED_CALLSITE: LogCallsite = LogCallsite {
            level: LogLevel::Debug,
            module_path: "kunit::captured",
            file: "mod.rs",
            line: 1,
        };
        __klog(
            &CAPTURED_CALLSITE,
            captured,
            format_args!("captured-policy"),
            true,
        );
        assert_eq!(
            KERNEL_LOG.iter_weak().last().unwrap().1.message(),
            "captured-policy"
        );

        EVALUATED.store(0, Ordering::Relaxed);
        kdebugln!(noprint, "{}", evaluated_argument());
        assert_eq!(EVALUATED.load(Ordering::Relaxed), 0);
        set_policy(initial);
    }

    #[kunit]
    fn long_utf8_record_stays_visible_and_marks_truncation() {
        let mut message = "界".repeat(170);
        message.push('\n');
        message.push('界');
        kemerg!("{message}");

        let (_, record) = KERNEL_LOG.iter_weak().last().unwrap();
        assert!(record.flags.contains(LogRecordFlags::TRUNCATED));
        assert!(record.message().ends_with('\n'));
        assert!(record.message().is_char_boundary(record.message().len()));
    }
}
