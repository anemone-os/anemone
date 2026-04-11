//! write system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/write.2.html

use core::ops::DerefMut;

use crate::prelude::{dt::UserReadPtr, *};

#[syscall(SYS_WRITE)]
fn sys_write(fd: usize, buf: UserReadPtr<u8>, count: usize) -> Result<u64, SysError> {
    let file = with_current_task(|task| task.get_fd(fd).ok_or(KernelError::BadFileDescriptor))?;
    let uspace = with_current_task(|task| {
        task.clone_uspace()
            .expect("user task should have a user space")
    });
    let slice = buf.slice(count);

    let mut kbuf = Vec::with_capacity(count);
    kbuf.resize(count, 0);

    let mut usp = uspace.write();
    let ptr = unsafe { slice.validate_with(usp.deref_mut())? };
    unsafe {
        (&mut kbuf)[..count].copy_from_slice(core::slice::from_raw_parts(ptr.cast::<u8>(), count));
    }

    let len = file.write(&kbuf[..count]).map(|n| n as u64)?;

    Ok(len)
}
