//! lseek system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/lseek.2.html

use crate::{prelude::*, syscall::handler::TryFromSyscallArg, task::files::Fd};

#[derive(Debug)]
enum SeekWhence {
    Set,
    Cur,
    End,
    Data,
    Hole,
}

impl TryFromSyscallArg for SeekWhence {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        use anemone_abi::fs::linux::seek::*;

        let whence = match raw as usize {
            SEEK_SET => Self::Set,
            SEEK_CUR => Self::Cur,
            SEEK_END => Self::End,
            SEEK_DATA => Self::Data,
            SEEK_HOLE => Self::Hole,
            _ => return Err(SysError::InvalidArgument),
        };

        if matches!(whence, Self::Data | Self::Hole) {
            knoticeln!("[NYI] sys_lseek: SEEK_DATA and SEEK_HOLE are not supported yet");
            return Err(SysError::NotYetImplemented);
        }

        Ok(whence)
    }
}

#[syscall(SYS_LSEEK)]
fn sys_lseek(fd: Fd, offset: i64, whence: SeekWhence) -> Result<u64, SysError> {
    kdebugln!(
        "sys_lseek: fd={:?}, offset={}, whence={:?}",
        fd,
        offset,
        whence
    );

    let task = get_current_task();
    let fd = task.get_fd(fd)?;

    let vfs_file = fd.vfs_file();

    match whence {
        SeekWhence::Set => {
            if offset < 0 {
                return Err(SysError::InvalidArgument);
            }
            vfs_file.seek(offset as usize)?;
            Ok(offset as u64)
        },
        SeekWhence::Cur => {
            // TODO: make this atomic.
            let new_pos = vfs_file.pos() as i64 + offset;
            if new_pos < 0 {
                return Err(SysError::InvalidArgument);
            }
            let new_pos = new_pos as usize;
            vfs_file.seek(new_pos)?;
            Ok(new_pos as u64)
        },
        SeekWhence::End => {
            // TODO: make this atomic.
            // TODO: this is wrong for some file types (e.g. procfs files). should we
            // delegate the seek logic to the file system?
            let new_pos = vfs_file.inode().size() as i64 + offset;
            if new_pos < 0 {
                return Err(SysError::InvalidArgument);
            }
            let new_pos = new_pos as usize;
            vfs_file.seek(new_pos)?;
            Ok(new_pos as u64)
        },
        _ => unreachable!(/* handled above */),
    }
}
