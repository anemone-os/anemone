use core::fmt;

use nemophila_wasm::{Caller, Error, Linker, errors::HostError};

use crate::prelude::*;

use super::HostContext;

// WIT consumer: `nemophila/wit/nemophila.wit` / `logging`.
// Keep this handwritten Core Wasm lowering and its link/call tests in the same
// reviewed change as any canonical identity or value-shape revision.
pub(in crate::nemophila) const LOGGING_MODULE: &str = "anemone:nemophila/logging@0.1.0";
pub(in crate::nemophila) const LOGGING_WRITE: &str = "write";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::nemophila) enum LoggingFailure {
    InvalidLevel(i32),
    MissingMemory,
    RangeOverflow,
    OutOfBounds,
    InvalidUtf8,
}

impl fmt::Display for LoggingFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLevel(level) => write!(f, "invalid Nemophila log level {level}"),
            Self::MissingMemory => f.write_str("Nemophila logging caller has no exported memory"),
            Self::RangeOverflow => f.write_str("Nemophila log message range overflows"),
            Self::OutOfBounds => f.write_str("Nemophila log message is outside guest memory"),
            Self::InvalidUtf8 => f.write_str("Nemophila log message is not UTF-8"),
        }
    }
}

impl HostError for LoggingFailure {}

#[derive(Debug, Clone, Copy)]
enum GuestLogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

impl TryFrom<i32> for GuestLogLevel {
    type Error = LoggingFailure;

    fn try_from(raw: i32) -> Result<Self, LoggingFailure> {
        match raw {
            0 => Ok(Self::Debug),
            1 => Ok(Self::Info),
            2 => Ok(Self::Warning),
            3 => Ok(Self::Error),
            _ => Err(LoggingFailure::InvalidLevel(raw)),
        }
    }
}

pub(in crate::nemophila) fn install(linker: &mut Linker<HostContext>) -> Result<(), Error> {
    linker.func_wrap(
        LOGGING_MODULE,
        LOGGING_WRITE,
        |caller: Caller<'_, HostContext>, level: i32, pointer: i32, length: i32| {
            write(caller, level, pointer, length)
        },
    )?;
    Ok(())
}

fn write(
    caller: Caller<'_, HostContext>,
    level: i32,
    pointer: i32,
    length: i32,
) -> Result<(), Error> {
    let level = GuestLogLevel::try_from(level).map_err(Error::host)?;
    // Canonical ABI represents wasm32 pointers and lengths in i32 values, but
    // their bit patterns are unsigned. Bounds are checked against the owning
    // guest memory before the borrowed message reaches printk.
    let pointer =
        usize::try_from(pointer as u32).map_err(|_| Error::host(LoggingFailure::OutOfBounds))?;
    let length =
        usize::try_from(length as u32).map_err(|_| Error::host(LoggingFailure::OutOfBounds))?;
    let end = pointer
        .checked_add(length)
        .ok_or_else(|| Error::host(LoggingFailure::RangeOverflow))?;
    let memory = caller
        .get_export("memory")
        .and_then(|export| export.into_memory())
        .ok_or_else(|| Error::host(LoggingFailure::MissingMemory))?;
    let message = memory
        .data(&caller)
        .get(pointer..end)
        .ok_or_else(|| Error::host(LoggingFailure::OutOfBounds))?;
    let message =
        core::str::from_utf8(message).map_err(|_| Error::host(LoggingFailure::InvalidUtf8))?;

    match level {
        GuestLogLevel::Debug => {
            kdebug!("{message}");
        },
        GuestLogLevel::Info => {
            kinfo!("{message}");
        },
        GuestLogLevel::Warning => {
            kwarning!("{message}");
        },
        GuestLogLevel::Error => {
            kerr!("{message}");
        },
    }
    Ok(())
}
