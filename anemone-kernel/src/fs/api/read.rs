//! read system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/read.2.html

use core::ops::DerefMut;

use crate::{
    prelude::{dt::UserWritePtr, *},
    task::files::Fd,
};

#[syscall(SYS_READ)]
fn sys_read(fd: Fd, buf: UserWritePtr<u8>, count: usize) -> Result<u64, SysError> {
    if count == 0 {
        return Ok(0);
    }

    let (file, uspace) = {
        let task = get_current_task();
        let file = task.get_fd(fd).ok_or(SysError::BadFileDescriptor)?;
        let uspace = task.clone_uspace();

        (file, uspace)
    };

    let slice = buf.slice(count);

    let mut kbuf = vec![0u8; count];

    let len = file.read(&mut kbuf[..count]).map(|n| n as u64)?;

    let mut usp = uspace.write();
    let ptr = unsafe { slice.validate_mut_with(usp.deref_mut())? };
    unsafe {
        ptr.cast::<u8>()
            .copy_from_nonoverlapping(kbuf.as_ptr(), len as usize);
    }

    Ok(len)
}
