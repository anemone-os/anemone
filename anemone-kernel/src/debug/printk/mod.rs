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
pub fn register_console(console: Arc<dyn Console>, flags: ConsoleFlags) {
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

mod klog {
    use core::fmt::{Arguments, Write};

    use crate::utils::writer::{BufferWriter, OverflowBehavior};

    use super::*;

    pub fn __klog(level: Option<LogLevel>, leveled_msg: Arguments) {
        let mut record = LogRecord::empty();
        let mut writer = BufferWriter::<{ OverflowBehavior::TRUNCATE }>::new(&mut record.msg);
        let _ = writer.write_fmt(leveled_msg);
        record.len = writer.pos();
        record.level = level;

        let full_msg_str =
            core::str::from_utf8(&record.msg[..record.len]).unwrap_or("[Invalid UTF-8]");
        SYS_CONSOLE.output(full_msg_str);
        KERNEL_LOG.append(record);
    }

    #[macro_export]
    macro_rules! kprint {
        ($level:ident, $($arg:tt)*) => {
            $crate::debug::printk::__klog(
                Some($crate::debug::printk::LogLevel::$level),
                format_args!(
                    "[{:>7}] {}",
                    $crate::debug::printk::LogLevel::$level.as_painted(),
                    format_args!($($arg)*)
                )
            );
        };
        ($($arg:tt)*) => {
            $crate::debug::printk::__klog(None, format_args!($($arg)*));
        };
    }

    #[macro_export]
    macro_rules! kprintln {
        () => {
            $crate::kprint!("\n");
        };
        ($level:ident, $($arg:tt)*) => {
            $crate::kprint!($level, "{}\n", format_args!($($arg)*));
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
                        ($dollar($args:tt)*) => {
                            $crate::kprint!($level, $dollar($args)*);
                        }
                    }

                    #[macro_export]
                    macro_rules! [<k $name ln>] {
                        () => {
                            $crate::kprint!($level, "\n");
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
