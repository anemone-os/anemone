//! write system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/write.2.html

use crate::prelude::{dt::UserReadPtr, *};

#[syscall(SYS_WRITE)]
fn sys_write(fd: usize, buf: UserReadPtr<u8>, count: usize) -> Result<u64, SysError> {
    with_current_task(|task| {
        let file = task.get_fd(fd).ok_or(KernelError::BadFileDescriptor)?;
        let slice = unsafe { buf.slice(count, task)? };
        file.write(unsafe { &*slice.as_slice_ptr() })
            .map(|n| n as u64)
            .map_err(Into::into)
    })
}
