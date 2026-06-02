use crate::{prelude::*, task::files::Fd};

#[syscall(SYS_FTRUNCATE)]
fn sys_ftruncate(fd: Fd, length: i64) -> Result<u64, SysError> {
    if length < 0 {
        return Err(SysError::InvalidArgument);
    }

    let task = get_current_task();
    let cred = task.cred();
    task.get_fd(fd)?.truncate(length as u64, &cred)?;

    Ok(0)
}
