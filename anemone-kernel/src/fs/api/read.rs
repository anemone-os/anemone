//! read system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/read.2.html

use core::ops::DerefMut;

use crate::prelude::{dt::UserWritePtr, *};

#[syscall(SYS_READ)]
fn sys_read(fd: usize, buf: UserWritePtr<u8>, count: usize) -> Result<u64, SysError> {
    if count == 0 {
        return Ok(0);
    }

    let file = with_current_task(|task| task.get_fd(fd).ok_or(KernelError::BadFileDescriptor))?;
    let uspace = with_current_task(|task| {
        task.clone_uspace()
            .expect("user task should have a user space")
    });
    let slice = buf.slice(count);

    let mut kbuf = Vec::with_capacity(count);
    kbuf.resize(count, 0);

    let len = file.read(&mut kbuf[..count]).map(|n| n as u64)?;

    let mut usp = uspace.write();
    let ptr = unsafe { slice.validate_mut_with(usp.deref_mut())? };
    unsafe {
        ptr.cast::<u8>()
            .copy_from_nonoverlapping(kbuf.as_ptr(), len as usize);
    }

    Ok(len)
}
