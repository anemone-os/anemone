//! eventfd2 system call.

use anemone_abi::{
    fs::linux::eventfd::{EFD_CLOEXEC, EFD_NONBLOCK, EFD_SEMAPHORE},
    syscall::SYS_EVENTFD2,
};

use crate::{
    fs::eventfd::create_eventfd,
    prelude::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        *,
    },
    task::files::{FdFlags, FileDescOps, FileStatusFlags, LinuxOpenCompat, OpenAccessMode},
};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct EventFdFlags: u32 {
        const SEMAPHORE = EFD_SEMAPHORE;
        const CLOEXEC = EFD_CLOEXEC;
        const NONBLOCK = EFD_NONBLOCK;
    }
}

impl TryFromSyscallArg for EventFdFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)?;
        Self::from_bits(raw).ok_or(SysError::InvalidArgument)
    }
}

#[syscall(SYS_EVENTFD2)]
fn sys_eventfd2(count: u32, flags: EventFdFlags) -> Result<u64, SysError> {
    let file = create_eventfd(count, flags.contains(EventFdFlags::SEMAPHORE))?;

    let mut status_flags = FileStatusFlags::empty();
    status_flags.set(
        FileStatusFlags::NONBLOCK,
        flags.contains(EventFdFlags::NONBLOCK),
    );
    file.check_status_flags(status_flags.to_file_op_status_flags())?;

    let fd_flags = if flags.contains(EventFdFlags::CLOEXEC) {
        FdFlags::CLOSE_ON_EXEC
    } else {
        FdFlags::empty()
    };

    let fd = get_current_task().open_fd_with_description_ops(
        file,
        OpenAccessMode::ReadWrite,
        status_flags,
        LinuxOpenCompat::empty(),
        fd_flags,
        FileDescOps {
            notification_suppressed: true,
            ..FileDescOps::default()
        },
    )?;

    Ok(fd.raw() as u64)
}
