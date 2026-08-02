//! fcntl system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/fcntl.2.html

mod posix_lock;

use crate::{
    fs::FileFcntlCmd,
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
    SetPipeSize,
    GetPipeSize,
}

impl FcntlCmd {
    const fn to_file_cmd(self) -> Option<FileFcntlCmd> {
        match self {
            Self::GetPipeSize => Some(FileFcntlCmd::GetPipeSize),
            Self::SetPipeSize => Some(FileFcntlCmd::SetPipeSize),
            _ => None,
        }
    }
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
            F_GETLK => Ok(Self::GetLk),
            F_SETLK => Ok(Self::SetLk),
            F_SETLKW => Ok(Self::SetLkw),
            F_GETOWN => Ok(Self::GetOwn),
            F_SETOWN => Ok(Self::SetOwn),
            F_GETSIG => Err(SysError::NotYetImplemented),
            F_SETSIG => Err(SysError::NotYetImplemented),
            F_DUPFD_CLOEXEC => Ok(Self::DupCloexec),
            F_SETPIPE_SZ => Ok(Self::SetPipeSize),
            F_GETPIPE_SZ => Ok(Self::GetPipeSize),
            _ => Err(SysError::InvalidArgument),
        };
        if ret.is_err() {
            knoticeln!("[NYI] fcntl command {} is not supported yet", raw);
        }
        ret
    }
}

fn parse_dup_min_fd(raw: u64) -> Result<Fd, SysError> {
    // F_DUPFD* owns a command-specific minimum, not an already-open fd.
    // Linux reports EINVAL for negative or otherwise unrepresentable minima.
    let minimum = raw as i32;
    if minimum < 0 {
        return Err(SysError::InvalidArgument);
    }
    Fd::new(minimum as u32).ok_or(SysError::InvalidArgument)
}

#[syscall(SYS_FCNTL)]
fn sys_fcntl(raw_fd: u64, cmd: FcntlCmd, arg: u64) -> Result<u64, SysError> {
    // Linux exposes command decoding before fd admission. Keep arg0 as an
    // infallible transport so the syscall wrapper parses `cmd` first, then
    // validate the fd here before dispatching the decoded command.
    let fd = Fd::try_from_syscall_arg(raw_fd)?;
    kdebugln!("fcntl: fd={:?}, cmd={:?}, arg={:#x}", fd, cmd, arg);

    let task = get_current_task();
    match cmd {
        FcntlCmd::Dup => {
            let min_fd = parse_dup_min_fd(arg)?;
            let new_fd = task.dup_ge_than(fd, min_fd, false)?;
            Ok(new_fd.raw() as u64)
        },
        FcntlCmd::DupCloexec => {
            let min_fd = parse_dup_min_fd(arg)?;
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
            let settable = FileStatusFlags::settable_from_linux_flags(raw_flags);
            file.replace_settable_file_flags(settable)?;
            Ok(0)
        },
        FcntlCmd::GetPipeSize | FcntlCmd::SetPipeSize => {
            let file = task.get_fd(fd)?;
            let file_cmd = cmd
                .to_file_cmd()
                .expect("pipe size fcntl commands must have a VFS command");
            let ctx = file.fcntl_ctx(file_cmd, arg)?;
            file.vfs_file().fcntl(ctx)
        },
        FcntlCmd::GetLk => posix_lock::get_lock(&task, fd, arg),
        FcntlCmd::SetLk => posix_lock::set_lock(&task, fd, arg),
        FcntlCmd::SetLkw => posix_lock::set_lock_waiting(&task, fd, arg),
        _ => {
            knoticeln!("[NYI] fcntl command {:?} is not supported yet", cmd);
            Err(SysError::NotYetImplemented)
        },
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn dup_minimum_has_command_specific_einval_boundary() {
        assert_eq!(parse_dup_min_fd(0), Ok(Fd::new(0).unwrap()));
        assert_eq!(
            parse_dup_min_fd(i32::MAX as u64),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(parse_dup_min_fd(u64::MAX), Err(SysError::InvalidArgument));
    }
}
