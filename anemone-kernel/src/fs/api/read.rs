//! read system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/read.2.html

use crate::prelude::{dt::UserWritePtr, *};

#[syscall(SYS_READ)]
fn sys_read(fd: usize, buf: UserWritePtr<u8>, count: usize) -> Result<u64, SysError> {
    with_current_task(|task| {
        let file = task.get_fd(fd).ok_or(KernelError::BadFileDescriptor)?;
        let mut slice = unsafe { buf.slice(count, task)? };
        file.read(unsafe { &mut *slice.as_mut_slice_ptr() })
            .map(|n| n as u64)
            .map_err(Into::into)
    })
}
