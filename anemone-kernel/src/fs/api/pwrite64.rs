//! pwrite64 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/pwrite.2.html

use crate::{
    prelude::{
        user_access::{UserReadSlice, user_addr},
        *,
    },
    task::files::Fd,
};

#[syscall(SYS_PWRITE64)]
fn sys_pwrite64(
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

    {
        let mut guard = uspace.lock();

        let slice = UserReadSlice::try_new(buf, count, &mut guard)?;
        slice.copy_to_slice(&mut kbuf);
    }

    let len = file.write_at(offset, &kbuf[..count]).map(|n| n as u64)?;

    Ok(len)
}
