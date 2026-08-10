use anemone_abi::syscall::{
    DBG_LOG_CTL_CONSOLE_LEVEL_SHIFT, DBG_LOG_CTL_GET_LEVELS, DBG_LOG_CTL_LEVEL_MASK,
    DBG_LOG_CTL_RECORD_LEVEL_SHIFT, DBG_LOG_CTL_SET_LEVELS,
};

use super::*;
use sys::anemone::debug;

pub mod perf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogLevels {
    pub record: u8,
    pub console: u8,
}

impl LogLevels {
    pub const fn new(record: u8, console: u8) -> Self {
        Self { record, console }
    }

    const fn packed(self) -> u64 {
        ((self.record as u64) << DBG_LOG_CTL_RECORD_LEVEL_SHIFT)
            | ((self.console as u64) << DBG_LOG_CTL_CONSOLE_LEVEL_SHIFT)
    }

    const fn from_packed(raw: u64) -> Self {
        Self {
            record: ((raw >> DBG_LOG_CTL_RECORD_LEVEL_SHIFT) & DBG_LOG_CTL_LEVEL_MASK) as u8,
            console: ((raw >> DBG_LOG_CTL_CONSOLE_LEVEL_SHIFT) & DBG_LOG_CTL_LEVEL_MASK) as u8,
        }
    }
}

pub fn get_log_levels() -> Result<LogLevels, Errno> {
    debug::dbg_log_ctl(DBG_LOG_CTL_GET_LEVELS, 0).map(LogLevels::from_packed)
}

/// Replace both effective levels atomically and return the previous pair.
/// Invalid level values or relations are intentionally left for the kernel ABI
/// to reject with `EINVAL`.
pub fn set_log_levels(levels: LogLevels) -> Result<LogLevels, Errno> {
    debug::dbg_log_ctl(DBG_LOG_CTL_SET_LEVELS, levels.packed()).map(LogLevels::from_packed)
}
