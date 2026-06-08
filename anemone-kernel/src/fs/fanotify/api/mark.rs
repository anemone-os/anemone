use anemone_abi::fs::linux::fanotify as abi;

use crate::{
    prelude::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        user_access::SyscallArgValidatorExt,
        *,
    },
    syscall::user_access::user_addr,
    task::files::Fd,
};

const KNOWN_MARK_FLAGS: u32 = abi::FAN_MARK_ADD
    | abi::FAN_MARK_REMOVE
    | abi::FAN_MARK_DONT_FOLLOW
    | abi::FAN_MARK_ONLYDIR
    | abi::FAN_MARK_MOUNT
    | abi::FAN_MARK_IGNORED_MASK
    | abi::FAN_MARK_IGNORED_SURV_MODIFY
    | abi::FAN_MARK_FLUSH
    | abi::FAN_MARK_FILESYSTEM
    | abi::FAN_MARK_EVICTABLE
    | abi::FAN_MARK_IGNORE;

const STAGE_A_SUPPORTED_MASK: u64 = 0;
const STAGE_A_DEFERRED_MASK: u64 = abi::FAN_ACCESS
    | abi::FAN_MODIFY
    | abi::FAN_ATTRIB
    | abi::FAN_CLOSE_WRITE
    | abi::FAN_CLOSE_NOWRITE
    | abi::FAN_OPEN
    | abi::FAN_MOVED_FROM
    | abi::FAN_MOVED_TO
    | abi::FAN_CREATE
    | abi::FAN_DELETE
    | abi::FAN_DELETE_SELF
    | abi::FAN_MOVE_SELF
    | abi::FAN_OPEN_EXEC
    | abi::FAN_Q_OVERFLOW
    | abi::FAN_FS_ERROR
    | abi::FAN_OPEN_PERM
    | abi::FAN_ACCESS_PERM
    | abi::FAN_OPEN_EXEC_PERM
    | abi::FAN_EVENT_ON_CHILD
    | abi::FAN_RENAME
    | abi::FAN_ONDIR;

#[derive(Debug, Clone, Copy)]
pub struct RawFanotifyMarkFlags(u32);

impl TryFromSyscallArg for RawFanotifyMarkFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)?;
        if raw & !KNOWN_MARK_FLAGS != 0 {
            return Err(SysError::InvalidArgument);
        }
        Ok(Self(raw))
    }
}

#[syscall(SYS_FANOTIFY_MARK)]
pub fn sys_fanotify_mark(
    fanotify_fd: Fd,
    flags: RawFanotifyMarkFlags,
    mask: u64,
    dfd: i32,
    #[validate_with(user_addr.nullable())] pathname: Option<VirtAddr>,
) -> Result<u64, SysError> {
    let _ = (fanotify_fd, dfd, pathname);
    if mask & !(STAGE_A_SUPPORTED_MASK | STAGE_A_DEFERRED_MASK) != 0 {
        knoticeln!(
            "fanotify_mark: unknown mask bits rejected bits={:#x}",
            mask & !(STAGE_A_SUPPORTED_MASK | STAGE_A_DEFERRED_MASK)
        );
        return Err(SysError::InvalidArgument);
    }

    if flags.0 & (abi::FAN_MARK_EVICTABLE | abi::FAN_MARK_IGNORE) != 0 {
        knoticeln!(
            "fanotify_mark: deferred mark flags rejected bits={:#x}",
            flags.0 & (abi::FAN_MARK_EVICTABLE | abi::FAN_MARK_IGNORE)
        );
        return Err(SysError::InvalidArgument);
    }

    super::super::registry::reject_until_registry_gate()?;
    Ok(0)
}
