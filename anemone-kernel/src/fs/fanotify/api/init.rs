use anemone_abi::fs::linux::fanotify as abi;

use crate::{
    prelude::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        *,
    },
    task::files::{LinuxOpenCompat, OpenAccessMode},
};

use super::super::{
    file,
    group::FanGroup,
    types::{FanGroupMode, FanInitFlags, init_fd_flags, init_file_status_flags},
};

const SUPPORTED_INIT_FLAGS: u32 =
    abi::FAN_CLOEXEC | abi::FAN_NONBLOCK | abi::FAN_CLASS_CONTENT | abi::FAN_CLASS_PRE_CONTENT;

const DEFERRED_INIT_FLAGS: u32 = abi::FAN_UNLIMITED_QUEUE
    | abi::FAN_UNLIMITED_MARKS
    | abi::FAN_ENABLE_AUDIT
    | abi::FAN_REPORT_PIDFD
    | abi::FAN_REPORT_TID
    | abi::FAN_REPORT_FID
    | abi::FAN_REPORT_DIR_FID
    | abi::FAN_REPORT_NAME
    | abi::FAN_REPORT_TARGET_FID;

#[derive(Debug, Clone, Copy)]
pub struct RawFanotifyInitFlags(u32);

impl RawFanotifyInitFlags {
    fn parse(self) -> Result<ParsedInitFlags, SysError> {
        let raw = self.0;
        if raw & DEFERRED_INIT_FLAGS != 0 {
            knoticeln!(
                "fanotify_init: deferred init flags rejected bits={:#x}",
                raw & DEFERRED_INIT_FLAGS
            );
            return Err(SysError::InvalidArgument);
        }
        if raw & !(SUPPORTED_INIT_FLAGS | DEFERRED_INIT_FLAGS) != 0 {
            knoticeln!(
                "fanotify_init: unknown init flags rejected bits={:#x}",
                raw & !(SUPPORTED_INIT_FLAGS | DEFERRED_INIT_FLAGS)
            );
            return Err(SysError::InvalidArgument);
        }

        let mode = FanGroupMode::from_init_flags(raw)?;
        let init_flags = FanInitFlags::from_bits(raw & (abi::FAN_CLOEXEC | abi::FAN_NONBLOCK))
            .ok_or(SysError::InvalidArgument)?;

        Ok(ParsedInitFlags { mode, init_flags })
    }
}

impl TryFromSyscallArg for RawFanotifyInitFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        Ok(Self(syscall_arg_flag32(raw)?))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RawEventFdFlags(u32);

impl TryFromSyscallArg for RawEventFdFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        Ok(Self(syscall_arg_flag32(raw)?))
    }
}

#[derive(Debug, Clone, Copy)]
struct ParsedInitFlags {
    mode: FanGroupMode,
    init_flags: FanInitFlags,
}

#[syscall(SYS_FANOTIFY_INIT)]
pub fn sys_fanotify_init(
    flags: RawFanotifyInitFlags,
    event_f_flags: RawEventFdFlags,
) -> Result<u64, SysError> {
    let ParsedInitFlags { mode, init_flags } = flags.parse()?;

    if !get_current_task().has_cap(Capability::SYS_ADMIN) {
        // Only privileged path-fd listeners are implemented. Linux has a
        // restricted unprivileged FID-only mode, but FID reporting is deferred,
        // so creating a partial unprivileged listener would be a false success.
        return Err(SysError::PermissionDenied);
    }

    let event_fd_template = super::super::types::parse_event_fd_template(event_f_flags.0)?;
    let group = FanGroup::new(mode, event_fd_template);
    let file = file::open_group_file(group.clone())?;
    let task = get_current_task();
    let status = init_file_status_flags(init_flags);
    file.check_status_flags(status.to_file_op_status_flags())?;

    // `event_f_flags` is a template for event object fds, not the group fd
    // returned by fanotify_init(). Keep the group opened-description compat
    // state tied only to init flags.
    let compat = LinuxOpenCompat::empty();
    let fd = task.open_fd_with_description_ops(
        file,
        OpenAccessMode::ReadWrite,
        status,
        compat,
        init_fd_flags(init_flags),
        file::description_ops(),
    )?;

    kdebugln!(
        "fanotify_init: created group={:?} mode={:?} fd={}",
        group.id(),
        group.mode(),
        fd.raw(),
    );

    Ok(fd.raw() as u64)
}
