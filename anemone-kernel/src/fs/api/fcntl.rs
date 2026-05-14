//! fcntl system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/fcntl.2.html

use crate::{
    prelude::{handler::TryFromSyscallArg, *},
    task::files::Fd,
};

#[derive(Debug)]
enum FcntlCmd {
    Dup,
    GetFd,
    SetFd,
    GetFl,
    SetFl,
    GetLk,
    SetLk,
    SetLkw,
    GetOwn,
    SetOwn,
    GetSig,
    SetSig,
    // Linux-specific commands
    DupCloexec,
}

impl TryFromSyscallArg for FcntlCmd {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        use anemone_abi::fs::linux::fcntl::*;

        let raw = u32::try_from_syscall_arg(raw)?;
        let ret = match raw {
            F_DUPFD => Ok(Self::Dup),
            F_GETFD => Ok(Self::GetFd),
            F_SETFD => Ok(Self::SetFd),
            F_GETFL => Ok(Self::GetFl),
            F_SETFL => Ok(Self::SetFl),
            F_GETLK => Err(SysError::NotYetImplemented),
            F_SETLK => Err(SysError::NotYetImplemented),
            F_SETLKW => Err(SysError::NotYetImplemented),
            F_GETOWN => Ok(Self::GetOwn),
            F_SETOWN => Ok(Self::SetOwn),
            F_GETSIG => Err(SysError::NotYetImplemented),
            F_SETSIG => Err(SysError::NotYetImplemented),
            F_DUPFD_CLOEXEC => Ok(Self::DupCloexec),
            _ => Err(SysError::InvalidArgument),
        };
        if ret.is_err() {
            knoticeln!("[NYI] fcntl command {} is not supported yet", raw);
        }
        ret
    }
}

#[syscall(SYS_FCNTL)]
fn sys_fcntl(fd: Fd, cmd: FcntlCmd, arg: u64) -> Result<u64, SysError> {
    kdebugln!("fcntl: fd={:?}, cmd={:?}, arg={:#x}", fd, cmd, arg);

    let task = get_current_task();
    match cmd {
        FcntlCmd::Dup => {
            let min_fd = Fd::try_from_syscall_arg(arg)?;
            let new_fd = task
                .dup_ge_than(fd, min_fd, false)
                .ok_or(SysError::BadFileDescriptor)?;
            Ok(new_fd.raw() as u64)
        },
        FcntlCmd::DupCloexec => {
            let min_fd = Fd::try_from_syscall_arg(arg)?;
            let new_fd = task
                .dup_ge_than(fd, min_fd, true)
                .ok_or(SysError::BadFileDescriptor)?;
            Ok(new_fd.raw() as u64)
        },
        FcntlCmd::GetFd => {
            let file = task.get_fd(fd).ok_or(SysError::BadFileDescriptor)?;
            if file.fd_flags().contains(FdFlags::CLOSE_ON_EXEC) {
                Ok(1)
            } else {
                Ok(0)
            }
        },
        FcntlCmd::SetFd => {
            let file = task.get_fd(fd).ok_or(SysError::BadFileDescriptor)?;
            let close_on_exec = arg != 0;
            file.set_fd_flags(if close_on_exec {
                FdFlags::CLOSE_ON_EXEC
            } else {
                FdFlags::empty()
            });
            Ok(0)
        },
        FcntlCmd::GetFl => {
            let file = task.get_fd(fd).ok_or(SysError::BadFileDescriptor)?;
            let flags = file.file_flags().to_linux_open_flags();
            Ok(flags as u64)
        },
        _ => {
            knoticeln!("[NYI] fcntl command {:?} is not supported yet", cmd);
            Err(SysError::NotYetImplemented)
        },
    }
}
