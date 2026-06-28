use anemone_abi::fs::linux::close_range::{CLOSE_RANGE_CLOEXEC, CLOSE_RANGE_UNSHARE};

use crate::prelude::{
    handler::{TryFromSyscallArg, syscall_arg_flag32},
    *,
};

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct CloseRangeFlags: u32 {
        const UNSHARE = CLOSE_RANGE_UNSHARE;
        const CLOEXEC = CLOSE_RANGE_CLOEXEC;
    }
}

impl TryFromSyscallArg for CloseRangeFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)?;
        Self::from_bits(raw).ok_or(SysError::InvalidArgument)
    }
}

#[syscall(SYS_CLOSE_RANGE)]
fn sys_close_range(first: u32, last: u32, flags: CloseRangeFlags) -> Result<u64, SysError> {
    if first > last {
        return Err(SysError::InvalidArgument);
    }

    let task = get_current_task();
    task.close_range(first, last, flags);
    Ok(0)
}
