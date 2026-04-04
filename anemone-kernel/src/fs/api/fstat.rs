//! fstat system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/fstat.2.html

use anemone_abi::fs::linux::stat::Stat;

use crate::prelude::{dt::UserWritePtr, *};

#[syscall(SYS_FSTAT)]
fn sys_fstat(fd: usize, statbuf: UserWritePtr<Stat>) -> Result<u64, SysError> {
    with_current_task(|task| {
        let fd = task.get_fd(fd).ok_or(KernelError::BadFileDescriptor)?;

        let stat = fd.vfs_file().get_attr()?;

        unsafe {
            statbuf.as_mut_ptr().write_unaligned(stat.into());
        }

        Ok(0)
    })
}
