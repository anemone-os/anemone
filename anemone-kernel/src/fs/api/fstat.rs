//! fstat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/fstat.2.html

use anemone_abi::fs::linux::stat::Stat;

use crate::{
    prelude::{dt::UserWritePtr, *},
    task::files::Fd,
};

#[syscall(SYS_FSTAT)]
fn sys_fstat(fd: Fd, statbuf: UserWritePtr<Stat>) -> Result<u64, SysError> {
    let fd = with_current_task(|task| task.get_fd(fd).ok_or(KernelError::BadFileDescriptor))?;
    let stat = fd.vfs_file().get_attr()?;
    statbuf.safe_write(stat.into())?;
    Ok(0)
}
