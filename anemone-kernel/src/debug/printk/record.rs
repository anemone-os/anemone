use core::fmt::{Arguments, Write};

use crate::{
    prelude::*,
    utils::writer::{BufferWriter, OverflowBehavior},
};

pub(super) const LOG_RECORD_SIZE: usize = 1 << LOG_RECORD_SHIFT_BYTES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum LogLevel {
    Emerg = 0,
    Alert = 1,
    Crit = 2,
    Err = 3,
    Warning = 4,
    Notice = 5,
    Info = 6,
    Debug = 7,
}

impl LogLevel {
    pub const fn compile_enabled(self) -> bool {
        (self as u8) <= RECORD_LOG_LEVEL
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Emerg => "EMERG",
            Self::Alert => "ALERT",
            Self::Crit => "CRIT",
            Self::Err => "ERR",
            Self::Warning => "WARNING",
            Self::Notice => "NOTICE",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
        }
    }
}

impl TryFrom<u8> for LogLevel {
    type Error = SysError;

    fn try_from(raw: u8) -> Result<Self, Self::Error> {
        match raw {
            0 => Ok(Self::Emerg),
            1 => Ok(Self::Alert),
            2 => Ok(Self::Crit),
            3 => Ok(Self::Err),
            4 => Ok(Self::Warning),
            5 => Ok(Self::Notice),
            6 => Ok(Self::Info),
            7 => Ok(Self::Debug),
            _ => Err(SysError::InvalidArgument),
        }
    }
}

#[derive(Debug)]
pub(crate) struct LogCallsite {
    pub(crate) level: LogLevel,
    pub(crate) module_path: &'static str,
    pub(crate) file: &'static str,
    pub(crate) line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BootTimestamp {
    Unavailable,
    Monotonic(u64),
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) struct LogRecordFlags: u8 {
        const TRUNCATED = 1 << 0;
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LogRecord {
    pub(super) callsite: &'static LogCallsite,
    pub(super) timestamp: BootTimestamp,
    pub(super) flags: LogRecordFlags,
    pub(super) len: usize,
    pub(super) msg: [u8; LOG_RECORD_SIZE],
}

impl LogRecord {
    pub(super) fn from_args(
        callsite: &'static LogCallsite,
        timestamp: BootTimestamp,
        msg: Arguments<'_>,
    ) -> Self {
        let mut record = Self {
            callsite,
            timestamp,
            flags: LogRecordFlags::empty(),
            len: 0,
            msg: [0; LOG_RECORD_SIZE],
        };
        let mut writer = BufferWriter::<{ OverflowBehavior::TRUNCATE }>::new(&mut record.msg);
        let _ = writer.write_fmt(msg);
        record.len = writer.pos();
        if writer.truncated() {
            record.flags |= LogRecordFlags::TRUNCATED;
        }
        record
    }

    pub(super) fn message(&self) -> &str {
        core::str::from_utf8(&self.msg[..self.len])
            .expect("printk record writer must preserve UTF-8 boundaries")
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    static CALLSITE: LogCallsite = LogCallsite {
        level: LogLevel::Info,
        module_path: "kunit::printk::record",
        file: "record.rs",
        line: 123,
    };

    #[kunit]
    fn record_preserves_metadata_and_utf8_prefix() {
        let exact = "x".repeat(LOG_RECORD_SIZE);
        let exact_record = LogRecord::from_args(
            &CALLSITE,
            BootTimestamp::Unavailable,
            format_args!("{exact}"),
        );
        assert_eq!(exact_record.callsite.module_path, "kunit::printk::record");
        assert_eq!(exact_record.callsite.file, "record.rs");
        assert_eq!(exact_record.callsite.line, 123);
        assert_eq!(exact_record.len, LOG_RECORD_SIZE);
        assert!(!exact_record.flags.contains(LogRecordFlags::TRUNCATED));
        assert_eq!(exact_record.message(), exact);
        assert!(!exact_record.message().contains("\x1b["));

        let multibyte = "界".repeat(LOG_RECORD_SIZE);
        let truncated = LogRecord::from_args(
            &CALLSITE,
            BootTimestamp::Monotonic(42),
            format_args!("{multibyte}"),
        );
        assert!(truncated.flags.contains(LogRecordFlags::TRUNCATED));
        assert!(truncated.len < LOG_RECORD_SIZE);
        assert!(truncated.message().chars().all(|ch| ch == '界'));
    }
}
