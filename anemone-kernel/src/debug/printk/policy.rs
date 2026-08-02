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
    use crate::{
        task::kthread::{KThreadBuilder, KThreadCtx},
        utils::any_opaque::AnyOpaque,
    };

    #[derive(Opaque)]
    struct PolicyReaderContext {
        first: LogPolicy,
        second: LogPolicy,
        ready: Arc<AtomicBool>,
        done: Arc<AtomicBool>,
    }

    fn policy_reader(_: KThreadCtx, opaque: AnyOpaque) -> i32 {
        let context = opaque
            .cast::<PolicyReaderContext>()
            .expect("invalid printk policy reader context");
        let mut valid = matches!(snapshot_policy(), observed if observed == context.first || observed == context.second);
        context.ready.store(true, Ordering::Release);

        while valid && !context.done.load(Ordering::Acquire) {
            let observed = snapshot_policy();
            valid = observed == context.first || observed == context.second;
            yield_now();
        }
        i32::from(!valid)
    }

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

    #[kunit]
    fn concurrent_snapshots_never_observe_split_levels() {
        if RECORD_LOG_LEVEL == 0 {
            return;
        }

        let first = LogPolicy::from_packed(0).unwrap();
        let second = LogPolicy::from_packed(1).unwrap();
        let initial = replace_policy(first);
        let ready = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let worker = KThreadBuilder::new("kunit:printk-policy-reader")
            .spawn(
                policy_reader,
                AnyOpaque::new(PolicyReaderContext {
                    first,
                    second,
                    ready: ready.clone(),
                    done: done.clone(),
                }),
            )
            .expect("failed to spawn printk policy reader");

        while !ready.load(Ordering::Acquire) {
            yield_now();
        }
        for iteration in 0..10_000 {
            replace_policy(if iteration % 2 == 0 { second } else { first });
            if iteration % 64 == 0 {
                yield_now();
            }
        }
        done.store(true, Ordering::Release);
        assert_eq!(worker.wait_exited(), 0);
        replace_policy(initial);
    }
}
