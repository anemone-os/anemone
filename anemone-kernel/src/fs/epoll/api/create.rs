use anemone_abi::{fs::linux::epoll::EPOLL_CLOEXEC, syscall::SYS_EPOLL_CREATE1};

use crate::{
    fs::epoll::{Epoll, create_epoll_file, teardown_epoll_file},
    prelude::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        *,
    },
    task::files::{
        FdFlags, FileDesc, FileDescOps, FileStatusFlags, LinuxOpenCompat, OpenAccessMode,
        OpenedFileFinalReleaseCtx,
    },
};

#[derive(Debug, Clone, Copy)]
struct EpollCreateFlags(u32);

impl TryFromSyscallArg for EpollCreateFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let flags = syscall_arg_flag32(raw)?;
        if flags & !EPOLL_CLOEXEC != 0 {
            knoticeln!("sys_epoll_create1: unknown flags {:#x}", flags);
            return Err(SysError::InvalidArgument);
        }
        Ok(Self(flags))
    }
}

fn epoll_final_release(ctx: OpenedFileFinalReleaseCtx<'_>) {
    assert!(
        ctx.notification_suppressed,
        "epoll description lost notification-suppression capability"
    );
    teardown_epoll_file(ctx.file);
}

#[syscall(SYS_EPOLL_CREATE1)]
fn sys_epoll_create1(flags: EpollCreateFlags) -> Result<u64, SysError> {
    let task = get_current_task();
    let reservation = task.reserve_fd()?;

    let epoll = Epoll::try_new()?;
    let file = create_epoll_file(epoll)?;
    let fd_flags = if flags.0 & EPOLL_CLOEXEC != 0 {
        FdFlags::CLOSE_ON_EXEC
    } else {
        FdFlags::empty()
    };
    let file_desc = FileDesc::new_opened(
        file,
        OpenAccessMode::ReadWrite,
        FileStatusFlags::empty(),
        LinuxOpenCompat::empty(),
        fd_flags,
        FileDescOps {
            final_release: Some(epoll_final_release),
            notification_suppressed: true,
            ..FileDescOps::default()
        },
    );

    Ok(reservation.commit(file_desc).raw() as u64)
}
