//! This module manages kernel logging.

use yansi::{Paint, Painted};

const LOG_RECORD_SIZE: usize = 1 << LOG_RECORD_SHIFT_BYTES;
const LOG_BUFFER_CAPACITY: usize = (1 << LOG_BUFFER_SHIFT_KB) * 1024 / LOG_RECORD_SIZE;

use crate::{prelude::*, utils::circular_log::CircularLog};

#[derive(Debug, Clone, Copy)]
pub enum LogLevel {
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
    /// Returns the string representation of the log level.
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Emerg => "EMERG",
            LogLevel::Alert => "ALERT",
            LogLevel::Crit => "CRIT",
            LogLevel::Err => "ERR",
            LogLevel::Warning => "WARNING",
            LogLevel::Notice => "NOTICE",
            LogLevel::Info => "INFO",
            LogLevel::Debug => "DEBUG",
        }
    }

    /// Returns a colored and styled representation of the log level for
    /// terminal output.
    pub fn as_painted(&self) -> Painted<&'static str> {
        match self {
            LogLevel::Emerg => self.as_str().red().bold(),
            LogLevel::Alert => self.as_str().red().bold(),
            LogLevel::Crit => self.as_str().red().bold(),
            LogLevel::Err => self.as_str().red(),
            LogLevel::Warning => self.as_str().yellow(),
            LogLevel::Notice => self.as_str().magenta(),
            LogLevel::Info => self.as_str().cyan(),
            LogLevel::Debug => self.as_str().green(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LogRecord {
    pub level: LogLevel,
    // timestamp: u64, // TODO: add timestamp when we have a clock source
    pub len: usize,
    pub msg: [u8; LOG_RECORD_SIZE],
}

impl LogRecord {
    pub const fn empty(level: LogLevel) -> Self {
        Self {
            level,
            len: 0,
            msg: [0; LOG_RECORD_SIZE],
        }
    }
}

#[derive(Debug)]
pub struct KernelLog {
    buffer: SpinLock<CircularLog<LogRecord, LOG_BUFFER_CAPACITY>>,
}

impl KernelLog {
    /// Creates a new `KernelLog` instance with an empty log buffer.
    pub const fn new() -> Self {
        Self {
            buffer: SpinLock::new(CircularLog::new()),
        }
    }

    /// Appends a log record to the kernel log buffer.
    pub fn append(&self, record: LogRecord) {
        let mut buffer = self.buffer.lock_irqsave();
        buffer.push(record);
    }

    /// Get an weak iterator of the current log records in the kernel log
    /// buffer.
    ///
    /// TODO: explain the potential inconsistency, which, however, is harmless
    /// for logging purposes.
    pub fn iter_weak(&self) -> IterWeak<'_> {
        IterWeak {
            log: self,
            cur_seq: self.buffer.lock_irqsave().oldest_seq(),
        }
    }
}

#[derive(Debug)]
pub struct IterWeak<'a> {
    log: &'a KernelLog,
    cur_seq: usize,
}

impl Iterator for IterWeak<'_> {
    type Item = LogRecord;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let buf = self.log.buffer.lock_irqsave();
            if self.cur_seq >= buf.head_seq() {
                return None;
            }
            match buf.get_at(self.cur_seq) {
                Ok(record) => {
                    self.cur_seq += 1;
                    return Some(record);
                },
                Err(circular_log::ReadErr::Overwritten) => {
                    self.cur_seq = buf.oldest_seq();
                    core::hint::spin_loop();
                },
                Err(circular_log::ReadErr::NotReached) => {
                    unreachable!("we've checked that cur_seq < head_seq")
                },
            }
        }
    }
}
