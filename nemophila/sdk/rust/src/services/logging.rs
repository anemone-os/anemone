use alloc::fmt::format;
use core::{fmt::Arguments, marker::PhantomData};

use crate::__bindings;

/// A value-only logging window. It carries no Host resource handle.
pub struct Logging<'call> {
    _call: PhantomData<&'call mut ()>,
}

impl Logging<'_> {
    pub(crate) fn new() -> Self {
        Self { _call: PhantomData }
    }

    /// Submits one diagnostic record to the Host logging owner.
    pub fn write(&mut self, level: LogLevel, message: &str) {
        __bindings::anemone::nemophila::logging::write(level.into(), message);
    }

    /// Emits one raw console-only fragment using kernel `kprint` semantics.
    pub fn print(&mut self, message: &str) {
        __bindings::anemone::nemophila::logging::print(message);
    }

    /// Emits one raw console-only line using kernel `kprintln` semantics.
    pub fn println(&mut self, message: &str) {
        __bindings::anemone::nemophila::logging::println(message);
    }

    #[doc(hidden)]
    pub fn print_fmt(&mut self, arguments: Arguments<'_>) {
        self.print(&format(arguments));
    }

    #[doc(hidden)]
    pub fn println_fmt(&mut self, arguments: Arguments<'_>) {
        self.println(&format(arguments));
    }
}

/// Guest-visible logging severity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogLevel {
    Emergency,
    Alert,
    Critical,
    Error,
    Warning,
    Notice,
    Info,
    Debug,
}

impl From<LogLevel> for __bindings::anemone::nemophila::logging::Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Emergency => Self::Emergency,
            LogLevel::Alert => Self::Alert,
            LogLevel::Critical => Self::Critical,
            LogLevel::Error => Self::Error,
            LogLevel::Warning => Self::Warning,
            LogLevel::Notice => Self::Notice,
            LogLevel::Info => Self::Info,
            LogLevel::Debug => Self::Debug,
        }
    }
}
