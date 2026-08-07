use anemone_abi::syscall::{
    DBG_LOG_CTL_CONSOLE_LEVEL_SHIFT, DBG_LOG_CTL_LEVEL_MASK, DBG_LOG_CTL_RECORD_LEVEL_SHIFT,
    DBG_LOG_CTL_RESERVED_MASK,
};

use crate::prelude::*;

use super::LogLevel;

static_assert!(
    RECORD_LOG_LEVEL <= LogLevel::Debug as u8,
    "RECORD_LOG_LEVEL exceeds the supported log-level domain"
);
static_assert!(
    PRINT_LOG_LEVEL <= RECORD_LOG_LEVEL,
    "PRINT_LOG_LEVEL must not exceed RECORD_LOG_LEVEL"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LogPolicy {
    record_level: LogLevel,
    console_level: LogLevel,
}

impl LogPolicy {
    const fn configured_level(raw: u8) -> LogLevel {
        match raw {
            0 => LogLevel::Emerg,
            1 => LogLevel::Alert,
            2 => LogLevel::Crit,
            3 => LogLevel::Err,
            4 => LogLevel::Warning,
            5 => LogLevel::Notice,
            6 => LogLevel::Info,
            7 => LogLevel::Debug,
            _ => panic!("configured printk level is outside the supported domain"),
        }
    }

    const fn compile_default() -> Self {
        Self {
            record_level: Self::configured_level(RECORD_LOG_LEVEL),
            console_level: Self::configured_level(PRINT_LOG_LEVEL),
        }
    }

    pub fn from_packed(raw: u64) -> Result<Self, SysError> {
        if raw & DBG_LOG_CTL_RESERVED_MASK != 0 {
            return Err(SysError::InvalidArgument);
        }

        let record_level = LogLevel::try_from(
            ((raw >> DBG_LOG_CTL_RECORD_LEVEL_SHIFT) & DBG_LOG_CTL_LEVEL_MASK) as u8,
        )?;
        let console_level = LogLevel::try_from(
            ((raw >> DBG_LOG_CTL_CONSOLE_LEVEL_SHIFT) & DBG_LOG_CTL_LEVEL_MASK) as u8,
        )?;
        if console_level as u8 > record_level as u8 || record_level as u8 > RECORD_LOG_LEVEL {
            return Err(SysError::InvalidArgument);
        }

        Ok(Self {
            record_level,
            console_level,
        })
    }

    pub const fn packed(self) -> u64 {
        ((self.record_level as u64) << DBG_LOG_CTL_RECORD_LEVEL_SHIFT)
            | ((self.console_level as u64) << DBG_LOG_CTL_CONSOLE_LEVEL_SHIFT)
    }

    pub const fn records(self, level: LogLevel) -> bool {
        level as u8 <= self.record_level as u8
    }

    pub const fn prints(self, level: LogLevel) -> bool {
        level as u8 <= self.console_level as u8
    }

    pub const fn record_level(self) -> LogLevel {
        self.record_level
    }

    pub const fn console_level(self) -> LogLevel {
        self.console_level
    }
}

const DEFAULT_POLICY: LogPolicy = LogPolicy::compile_default();

// Both effective levels are one atomic policy word. A logger loads this once
// before constructing format arguments, while SET swaps the complete pair.
static LOG_POLICY: AtomicU16 = AtomicU16::new(DEFAULT_POLICY.packed() as u16);

pub fn snapshot_policy() -> LogPolicy {
    LogPolicy::from_packed(LOG_POLICY.load(Ordering::Acquire) as u64)
        .expect("stored printk policy must remain valid")
}

pub(crate) fn replace_policy(policy: LogPolicy) -> LogPolicy {
    let old = LOG_POLICY.swap(policy.packed() as u16, Ordering::AcqRel);
    LogPolicy::from_packed(old as u64).expect("stored printk policy must remain valid")
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn packed_policy_rejects_every_invalid_relation() {
        assert_eq!(
            LogPolicy::from_packed(1 << 16),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(LogPolicy::from_packed(8), Err(SysError::InvalidArgument));
        assert_eq!(
            LogPolicy::from_packed(8 << 8),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(
            LogPolicy::from_packed(6 | (7 << 8)),
            Err(SysError::InvalidArgument)
        );
        if RECORD_LOG_LEVEL < LogLevel::Debug as u8 {
            assert_eq!(
                LogPolicy::from_packed((RECORD_LOG_LEVEL + 1) as u64),
                Err(SysError::InvalidArgument)
            );
        }
    }
}
