//! write system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/write.2.html

use crate::{
    prelude::{
        user_access::{UserReadSlice, user_addr},
        *,
    },
    task::files::Fd,
};

#[syscall(SYS_WRITE)]
fn sys_write(
    fd: Fd,
    #[validate_with(user_addr)] buf: VirtAddr,
    count: usize,
) -> Result<u64, SysError> {
    if count == 0 {
        return Ok(0);
    }

    let (file, uspace) = {
        let task = get_current_task();
        let file = task.get_fd(fd).ok_or(SysError::BadFileDescriptor)?;
        let uspace = task.clone_uspace();

        (file, uspace)
    };

    let mut kbuf = vec![0u8; count];

    let mut guard = uspace.write();

    let slice = UserReadSlice::try_new(buf, count, &mut guard)?;
    slice.copy_to_slice(&mut kbuf);

    let len = file.write(&kbuf[..count]).map(|n| n as u64)?;

    Ok(len)
}
