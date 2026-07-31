//! timerfd system calls.

mod create;
mod gettime;
mod settime;

use anemone_abi::time::linux::timerfd::{
    TFD_CLOEXEC, TFD_NONBLOCK, TFD_TIMER_ABSTIME, TFD_TIMER_CANCEL_ON_SET,
};

use crate::{
    fs::timerfd::TimerFdSettimeFlags,
    prelude::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        *,
    },
};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) struct TimerFdCreateFlags: u32 {
        const CLOEXEC = TFD_CLOEXEC;
        const NONBLOCK = TFD_NONBLOCK;
    }
}

impl TryFromSyscallArg for TimerFdCreateFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)?;
        Self::from_bits(raw).ok_or(SysError::InvalidArgument)
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) struct TimerFdSettimeSysFlags: u32 {
        const ABSTIME = TFD_TIMER_ABSTIME;
        const CANCEL_ON_SET = TFD_TIMER_CANCEL_ON_SET;
    }
}

impl TryFromSyscallArg for TimerFdSettimeSysFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)?;
        Self::from_bits(raw).ok_or(SysError::InvalidArgument)
    }
}

impl From<TimerFdSettimeSysFlags> for TimerFdSettimeFlags {
    fn from(value: TimerFdSettimeSysFlags) -> Self {
        Self {
            abstime: value.contains(TimerFdSettimeSysFlags::ABSTIME),
            cancel_on_set: value.contains(TimerFdSettimeSysFlags::CANCEL_ON_SET),
        }
    }
}
