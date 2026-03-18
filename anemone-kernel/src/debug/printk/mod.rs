//! Kernel logging and console related functionality.

mod console;
pub use console::{Console, ConsoleFlags, SysConsole};
use spin::lazy::Lazy;
mod log;
pub use log::{LogLevel, LogRecord};

use crate::prelude::*;
use log::KernelLog;

static SYS_CONSOLE: Lazy<SysConsole> = Lazy::new(|| SysConsole::new());
static KERNEL_LOG: KernelLog = KernelLog::new();

/// Registers a console to receive log output with the specified flags.
pub fn register_console(console: Box<dyn Console>, flags: ConsoleFlags) {
    if flags.contains(ConsoleFlags::REPLAY) {
        let it = KERNEL_LOG.iter_weak();
        for record in it {
            let full_msg_str =
                core::str::from_utf8(&record.msg[..record.len]).unwrap_or("[Invalid UTF-8]");
            console.output(full_msg_str);
        }
    }

    SYS_CONSOLE.register_console(console, flags);
}

/// If there are any non-early consoles registered but none of them is enabled,
/// enable the first one. If there are only early consoles registered, keep them
/// as is and print a warning message, as this might indicate a problem with the
/// console registration order.
///
/// # Safety
///
/// Timing.
pub unsafe fn on_system_boot() {
    unsafe {
        let has_normal_con = SYS_CONSOLE.on_system_boot();
        if !has_normal_con {
            kwarningln!("no normal console registered, only early consoles are available");
        } else {
            kinfoln!("normal console(s) registered, early consoles have been unregistered");
        }
    }
}

mod klog {
    use core::fmt::{Arguments, Write};

    use crate::utils::writer::{BufferWriter, OverflowBehavior};

    use super::*;

    /// Internal function to log a message.
    ///
    /// If `level` is `Some`, the message will be logged with the specified log
    /// level.
    ///
    /// If `noprint` is `true`, the message will not be printed to the console,
    /// but will still be stored in the kernel log buffer if `level` is `Some`.
    pub fn __klog(level: Option<LogLevel>, msg: Arguments, noprint: bool) {
        match level {
            Some(level) => {
                let mut record = LogRecord::empty(level);
                let mut writer =
                    BufferWriter::<{ OverflowBehavior::TRUNCATE }>::new(&mut record.msg);
                let _ = writer.write_fmt(msg);
                record.len = writer.pos();
                let full_msg_str =
                    core::str::from_utf8(&record.msg[..record.len]).unwrap_or("[Invalid UTF-8]");

                if !noprint {
                    SYS_CONSOLE.output(full_msg_str);
                }

                KERNEL_LOG.append(record);
            },
            None => {
                if noprint {
                    return;
                }

                let mut record = LogRecord::empty(LogLevel::Debug);
                let mut writer =
                    BufferWriter::<{ OverflowBehavior::TRUNCATE }>::new(&mut record.msg);
                let _ = writer.write_fmt(msg);
                record.len = writer.pos();

                let full_msg_str =
                    core::str::from_utf8(&record.msg[..record.len]).unwrap_or("[Invalid UTF-8]");

                SYS_CONSOLE.output(full_msg_str);
            },
        }
    }

    #[macro_export]
    macro_rules! kprint {
        (noprint, $level:ident, $($arg:tt)*) => {
            $crate::debug::printk::__klog(
                Some($crate::debug::printk::LogLevel::$level),
                format_args!(
                    "[{:>7}] {}",
                    $crate::debug::printk::LogLevel::$level.as_painted(),
                    format_args!($($arg)*),
                ),
                true,
            );
        };
        ($level:ident, $($arg:tt)*) => {
            $crate::debug::printk::__klog(
                Some($crate::debug::printk::LogLevel::$level),
                format_args!(
                    "[{:>7}] {}",
                    $crate::debug::printk::LogLevel::$level.as_painted(),
                    format_args!($($arg)*),
                ),
                false,
            );
        };
        (noprint, $($arg:tt)*) => {
            $crate::debug::printk::__klog(None, format_args!($($arg)*), true);
        };
        ($($arg:tt)*) => {
            $crate::debug::printk::__klog(None, format_args!($($arg)*), false);
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

    // We could use the unstable feature 'macro_metavar_expr' to avoid the hacky
    // workaround of passing `$` as an argument to the macro.
    // But, whatever.
    // This is good enough for now.

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
}
pub use klog::__klog;
