use core::marker::PhantomData;

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
}

/// Guest-visible logging severity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

impl From<LogLevel> for __bindings::anemone::nemophila::logging::Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Debug => Self::Debug,
            LogLevel::Info => Self::Info,
            LogLevel::Warning => Self::Warning,
            LogLevel::Error => Self::Error,
        }
    }
}
