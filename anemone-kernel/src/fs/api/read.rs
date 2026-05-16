//! read system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/read.2.html

use crate::{
    prelude::{
        user_access::{UserWriteSlice, user_addr},
        *,
    },
    task::files::Fd,
};

#[syscall(SYS_READ)]
fn sys_read(
    fd: Fd,
    #[validate_with(user_addr)] buf: VirtAddr,
    count: usize,
) -> Result<u64, SysError> {
    if count == 0 {
        return Ok(0);
    }

    let (file, uspace) = {
        let task = get_current_task();
        let file = task.get_fd(fd)?;
        let uspace = task.clone_uspace_handle();

        (file, uspace)
    };

    let mut kbuf = vec![0u8; count];
    let len = file.read(&mut kbuf[..count]).map(|n| n as u64)?;

    let mut guard = uspace.lock();
    let mut slice = UserWriteSlice::try_new(buf, count, &mut guard)?;
    slice.copy_from_slice(&kbuf[..len as usize]);

    Ok(len)
}
