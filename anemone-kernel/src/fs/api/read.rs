//! read system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/read.2.html

use core::ops::DerefMut;

use crate::prelude::{dt::UserWritePtr, *};

#[syscall(SYS_READ)]
fn sys_read(fd: usize, buf: UserWritePtr<u8>, count: usize) -> Result<u64, SysError> {
    with_current_task(|task| -> Result<u64, SysError> {
        let file = task.get_fd(fd).ok_or(KernelError::BadFileDescriptor)?;
        let slice = buf.slice(count);
        let uspace = task
            .clone_uspace()
            .expect("user task should have a user space");
        let mut usp = uspace.write();
        let ptr = unsafe { slice.validate_with_mut(usp.deref_mut())? };
        let len = file.read(unsafe { &mut *ptr }).map(|n| n as u64)?;
        drop(usp);
        Ok(len)
    })
}
