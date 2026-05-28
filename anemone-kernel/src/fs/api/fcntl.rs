//! fcntl system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/fcntl.2.html

use crate::{
    prelude::{handler::TryFromSyscallArg, *},
    task::files::{Fd, FileStatusFlags},
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
    SetPipeSz,
    GetPipeSz,
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
            F_SETPIPE_SZ => Ok(Self::SetPipeSz),
            F_GETPIPE_SZ => Ok(Self::GetPipeSz),
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
            let new_fd = task.dup_ge_than(fd, min_fd, false)?;
            Ok(new_fd.raw() as u64)
        },
        FcntlCmd::DupCloexec => {
            let min_fd = Fd::try_from_syscall_arg(arg)?;
            let new_fd = task.dup_ge_than(fd, min_fd, true)?;
            Ok(new_fd.raw() as u64)
        },
        FcntlCmd::GetFd => {
            let file = task.get_fd(fd)?;
            if file.fd_flags().contains(FdFlags::CLOSE_ON_EXEC) {
                Ok(1)
            } else {
                Ok(0)
            }
        },
        FcntlCmd::SetFd => {
            let file = task.get_fd(fd)?;
            let close_on_exec = arg != 0;
            file.set_fd_flags(if close_on_exec {
                FdFlags::CLOSE_ON_EXEC
            } else {
                FdFlags::empty()
            });
            Ok(0)
        },
        FcntlCmd::GetFl => {
            let file = task.get_fd(fd)?;
            let flags = file.to_linux_getfl_flags();
            Ok(flags as u64)
        },
        FcntlCmd::SetFl => {
            use anemone_abi::fs::linux::open::{O_DSYNC, O_NOATIME, O_SYNC};

            let file = task.get_fd(fd)?;
            // Linux treats O_PATH file descriptions as path handles, not as
            // mutable file descriptions for F_SETFL.
            if file.is_path_only() {
                return Err(SysError::BadFileDescriptor);
            }

            let raw_flags = u32::try_from_syscall_arg(arg)?;
            let ignored = raw_flags & (O_DSYNC | O_SYNC | O_NOATIME);
            if ignored != 0 {
                knoticeln!(
                    "fcntl(F_SETFL): ignoring non-settable status flags: {:#x}",
                    ignored
                );
            }

            // F_SETFL can change only a narrow dynamic subset. Access mode,
            // O_PATH, O_CLOEXEC, creation flags, and saved compatibility bits
            // stay fixed on the open file description / fd.
            let mut flags = file.file_flags();
            let settable = FileStatusFlags::settable_from_linux_flags(raw_flags);
            flags.set(
                FileStatusFlags::APPEND,
                settable.contains(FileStatusFlags::APPEND),
            );
            flags.set(
                FileStatusFlags::NONBLOCK,
                settable.contains(FileStatusFlags::NONBLOCK),
            );
            flags.set(
                FileStatusFlags::DIRECT,
                settable.contains(FileStatusFlags::DIRECT),
            );
            file.set_file_flags(flags);
            crate::fs::pipe::update_nonblock(
                file.vfs_file(),
                flags.contains(FileStatusFlags::NONBLOCK),
            );
            Ok(0)
        },
        FcntlCmd::GetPipeSz => {
            let file = task.get_fd(fd)?;
            Ok(crate::fs::pipe::capacity(file.vfs_file())? as u64)
        },
        FcntlCmd::SetPipeSz => {
            let file = task.get_fd(fd)?;
            Ok(crate::fs::pipe::set_capacity(file.vfs_file(), arg)? as u64)
        },
        _ => {
            knoticeln!("[NYI] fcntl command {:?} is not supported yet", cmd);
            Err(SysError::NotYetImplemented)
        },
    }
}
