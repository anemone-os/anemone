use crate::{prelude::*, utils::circular_log::CircularLog};

use super::{LogRecord, record::LOG_RECORD_SIZE};

const LOG_BUFFER_CAPACITY: usize = (1 << LOG_BUFFER_SHIFT_KB) * 1024 / LOG_RECORD_SIZE;

static_assert!(
    LOG_BUFFER_CAPACITY > 0,
    "kernel log must contain at least one record"
);

#[derive(Debug)]
pub(super) struct KernelLog<const N: usize = LOG_BUFFER_CAPACITY> {
    buffer: SpinLock<CircularLog<LogRecord, N>>,
}

impl<const N: usize> KernelLog<N> {
    pub(super) const fn new() -> Self {
        Self {
            buffer: SpinLock::new(CircularLog::new()),
        }
    }

    /// Append owns sequence allocation and returns the sequence assigned to
    /// this record. The sequence is not duplicated in `LogRecord`.
    pub(super) fn append(&self, record: LogRecord) -> usize {
        self.buffer.lock_irqsave().push(record)
    }

    /// Return a best-effort iterator. If its next record is overwritten, it
    /// resumes from the ring's then-current oldest sequence.
    pub(super) fn iter_weak(&self) -> IterWeak<'_, N> {
        IterWeak {
            log: self,
            cur_seq: self.buffer.lock_irqsave().oldest_seq(),
        }
    }
}

#[derive(Debug)]
pub(super) struct IterWeak<'a, const N: usize> {
    log: &'a KernelLog<N>,
    cur_seq: usize,
}

impl<const N: usize> Iterator for IterWeak<'_, N> {
    type Item = (usize, LogRecord);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let buf = self.log.buffer.lock_irqsave();
            if self.cur_seq >= buf.head_seq() {
                return None;
            }
            match buf.get_at(self.cur_seq) {
                Ok(record) => {
                    let sequence = self.cur_seq;
                    self.cur_seq += 1;
                    return Some((sequence, record));
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

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::debug::printk::{BootTimestamp, LogCallsite, LogLevel};

    static CALLSITE: LogCallsite = LogCallsite {
        level: LogLevel::Notice,
        module_path: "kunit::printk::ring",
        file: "ring.rs",
        line: 1,
    };

    fn record(message: &str) -> LogRecord {
        LogRecord::from_args(
            &CALLSITE,
            BootTimestamp::Unavailable,
            format_args!("{message}"),
        )
    }

    #[kunit]
    fn append_and_overwrite_keep_one_sequence_truth() {
        let log = KernelLog::<2>::new();
        assert_eq!(log.append(record("zero")), 0);
        assert_eq!(log.append(record("one")), 1);
        assert_eq!(log.append(record("two")), 2);

        let records = log.iter_weak().collect::<Vec<_>>();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].0, 1);
        assert_eq!(records[0].1.message(), "one");
        assert_eq!(records[1].0, 2);
        assert_eq!(records[1].1.message(), "two");
    }
}
