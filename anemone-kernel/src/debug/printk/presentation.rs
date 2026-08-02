use core::fmt::{self, Write};

use yansi::{Paint, Painted};

use crate::{device, prelude::*};

use super::{BootTimestamp, LogLevel, LogRecord, LogRecordFlags};

const TRUNCATION_MARKER: &str = " [truncated]";

impl LogLevel {
    fn as_painted(self) -> Painted<&'static str> {
        match self {
            Self::Emerg | Self::Alert | Self::Crit => self.as_str().red().bold(),
            Self::Err => self.as_str().red(),
            Self::Warning => self.as_str().yellow(),
            Self::Notice => self.as_str().magenta(),
            Self::Info => self.as_str().cyan(),
            Self::Debug => self.as_str().green(),
        }
    }
}

pub(super) fn write_record(
    writer: &mut dyn Write,
    _sequence: usize,
    record: &LogRecord,
) -> fmt::Result {
    match record.timestamp {
        BootTimestamp::Unavailable => writer.write_str("[    ?.??????] ")?,
        BootTimestamp::Monotonic(mono) => {
            let elapsed = duration_from_mono(mono);
            write!(
                writer,
                "[{:>5}.{:06}] ",
                elapsed.as_secs(),
                elapsed.subsec_micros()
            )?;
        },
    }
    write!(
        writer,
        "[{:>7}] {}:{}: ",
        record.callsite.level.as_painted(),
        record.callsite.module_path,
        record.callsite.line,
    )?;

    let message = record.message();
    if record.flags.contains(LogRecordFlags::TRUNCATED) && message.ends_with('\n') {
        writer.write_str(&message[..message.len() - 1])?;
        writer.write_str(TRUNCATION_MARKER)?;
        writer.write_char('\n')
    } else {
        writer.write_str(message)?;
        if record.flags.contains(LogRecordFlags::TRUNCATED) {
            writer.write_str(TRUNCATION_MARKER)?;
        }
        Ok(())
    }
}

pub(super) fn output_record(sequence: usize, record: &LogRecord) {
    device::console::output_with(|writer| write_record(writer, sequence, record));
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::debug::printk::LogCallsite;

    static CALLSITE: LogCallsite = LogCallsite {
        level: LogLevel::Info,
        module_path: "kunit::printk::presentation",
        file: "presentation.rs",
        line: 77,
    };

    #[kunit]
    fn live_and_replay_formatter_share_exact_presentation() {
        let mut record = LogRecord::from_args(
            &CALLSITE,
            BootTimestamp::Unavailable,
            format_args!("message\n"),
        );
        record.flags |= LogRecordFlags::TRUNCATED;

        let mut live = String::new();
        let mut replay = String::new();
        write_record(&mut live, 9, &record).unwrap();
        write_record(&mut replay, 9, &record).unwrap();

        assert_eq!(live, replay);
        assert!(live.starts_with("[    ?.??????] "));
        assert!(live.contains("kunit::printk::presentation:77: message [truncated]\n"));
        assert_eq!(record.message(), "message\n");
    }
}
