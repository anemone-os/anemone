//! lseek system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/lseek.2.html

use crate::{prelude::*, syscall::handler::TryFromSyscallArg, task::files::Fd};

#[derive(Debug)]
struct LseekFrom(SeekFrom);

impl TryFromSyscallArg for LseekFrom {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        use anemone_abi::fs::linux::seek::*;

        let whence = match raw as usize {
            SEEK_SET => SeekFrom::Set(0),
            SEEK_CUR => SeekFrom::Cur(0),
            SEEK_END => SeekFrom::End(0),
            SEEK_DATA | SEEK_HOLE => {
                knoticeln!("[NYI] sys_lseek: SEEK_DATA and SEEK_HOLE are not supported yet");
                return Err(SysError::NotYetImplemented);
            },
            _ => return Err(SysError::InvalidArgument),
        };

        Ok(Self(whence))
    }
}

#[syscall(SYS_LSEEK)]
fn sys_lseek(fd: Fd, offset: i64, whence: LseekFrom) -> Result<u64, SysError> {
    let from = match whence.0 {
        SeekFrom::Set(_) => SeekFrom::Set(offset),
        SeekFrom::Cur(_) => SeekFrom::Cur(offset),
        SeekFrom::End(_) => SeekFrom::End(offset),
    };

    kdebugln!(
        "sys_lseek: fd={:?}, offset={}, whence={:?}",
        fd,
        offset,
        from
    );

    let task = get_current_task();
    let fd = task.get_fd(fd)?;
    if fd.is_path_only() {
        return Err(SysError::BadFileDescriptor);
    }

    fd.seek(from).map(|pos| pos as u64)
}
