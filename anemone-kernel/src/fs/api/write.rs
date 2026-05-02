//! write system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/write.2.html

use core::ops::DerefMut;

use crate::{
    prelude::{dt::UserReadPtr, *},
    task::files::Fd,
};

#[syscall(SYS_WRITE)]
fn sys_write(fd: Fd, buf: UserReadPtr<u8>, count: usize) -> Result<u64, SysError> {
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

    let mut usp = uspace.write();
    let ptr = unsafe { slice.validate_with(usp.deref_mut())? };
    unsafe {
        (&mut kbuf)[..count].copy_from_slice(core::slice::from_raw_parts(ptr.cast::<u8>(), count));
    }

    let len = file.write(&kbuf[..count]).map(|n| n as u64)?;

    Ok(len)
}
