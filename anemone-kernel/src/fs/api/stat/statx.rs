use anemone_abi::fs::linux::{
    at::*,
    statx::{self as linux_statx, StatX},
};
use bitflags::bitflags;

use crate::{
    fs::api::{
        args::AtFd,
        stat::{args::StatAtFlag, kernel_fstatat},
    },
    prelude::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        user_access::{UserWritePtr, c_readonly_string, user_addr},
        *,
    },
};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct StatxAtFlag: u32 {
        const EMPTY_PATH = AT_EMPTY_PATH;
        const SYMLINK_NOFOLLOW = AT_SYMLINK_NOFOLLOW;
        const NO_AUTOMOUNT = AT_NO_AUTOMOUNT;
        const STATX_FORCE_SYNC = AT_STATX_FORCE_SYNC;
        const STATX_DONT_SYNC = AT_STATX_DONT_SYNC;
    }
}

impl TryFromSyscallArg for StatxAtFlag {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)?;
        let flags = Self::from_bits(raw).ok_or(SysError::InvalidArgument)?;

        if raw & AT_STATX_SYNC_TYPE == AT_STATX_SYNC_TYPE {
            return Err(SysError::InvalidArgument);
        }

        Ok(flags)
    }
}

impl From<StatxAtFlag> for StatAtFlag {
    fn from(flags: StatxAtFlag) -> Self {
        let mut ret = Self::empty();

        if flags.contains(StatxAtFlag::EMPTY_PATH) {
            ret |= Self::EMPTY_PATH;
        }
        if flags.contains(StatxAtFlag::SYMLINK_NOFOLLOW) {
            ret |= Self::SYMLINK_NOFOLLOW;
        }
        if flags.contains(StatxAtFlag::NO_AUTOMOUNT) {
            ret |= Self::NO_AUTOMOUNT;
        }

        ret
    }
}

#[syscall(SYS_STATX)]
fn sys_statx(
    dirfd: AtFd,
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] pathname: Box<str>,
    flags: StatxAtFlag,
    mask: u32,
    #[validate_with(user_addr)] statxbuf: VirtAddr,
) -> Result<u64, SysError> {
    if mask & linux_statx::RESERVED != 0 {
        return Err(SysError::InvalidArgument);
    }

    let kbuf = kernel_fstatat(dirfd, &pathname, flags.into())?.to_linux_statx(mask);

    let usp = get_current_task().clone_uspace_handle();
    let mut guard = usp.lock();

    let mut statxbuf = UserWritePtr::<StatX>::try_new(statxbuf, &mut guard)?;
    statxbuf.write(kbuf);

    Ok(0)
}
