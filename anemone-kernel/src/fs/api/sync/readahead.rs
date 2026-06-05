//! readahead system call.

use crate::{prelude::*, task::files::Fd};

#[syscall(SYS_READAHEAD)]
fn sys_readahead(raw_fd: i32, _offset: i64, count: usize) -> Result<u64, SysError> {
    if raw_fd < 0 {
        return Err(SysError::BadFileDescriptor);
    }

    if count > i64::MAX as usize {
        return Err(SysError::InvalidArgument);
    }

    let fd = Fd::new(raw_fd as u32).ok_or(SysError::BadFileDescriptor)?;
    let task = get_current_task();
    let file = task.get_fd(fd)?;

    if !file.can_read() {
        return Err(SysError::BadFileDescriptor);
    }

    if !matches!(
        file.vfs_file().inode().ty(),
        InodeType::Regular | InodeType::Block
    ) {
        return Err(SysError::InvalidArgument);
    }

    Ok(0)
}
