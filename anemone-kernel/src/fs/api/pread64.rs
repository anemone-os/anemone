//! pread64 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/pread.2.html

use crate::{
    prelude::{
        user_access::{UserWriteSlice, user_addr},
        *,
    },
    task::files::Fd,
};

#[syscall(SYS_PREAD64)]
fn sys_pread64(
    fd: Fd,
    #[validate_with(user_addr)] buf: VirtAddr,
    count: usize,
    offset: i64,
) -> Result<u64, SysError> {
    if count == 0 {
        return Ok(0);
    }

    let offset = if offset < 0 {
        return Err(SysError::InvalidArgument);
    } else {
        offset as usize
    };

    let (file, uspace) = {
        let task = get_current_task();
        let file = task.get_fd(fd)?;
        let uspace = task.clone_uspace_handle();

        (file, uspace)
    };

    let mut kbuf = vec![0u8; count];
    let len = file.read_at(offset, &mut kbuf[..count]).map(|n| n as u64)?;

    let mut guard = uspace.lock();
    let mut slice = UserWriteSlice::try_new(buf, count, &mut guard)?;
    slice.copy_from_slice(&kbuf[..len as usize]);

    Ok(len)
}
