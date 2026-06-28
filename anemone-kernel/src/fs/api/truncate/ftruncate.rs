use crate::{
    fs::fanotify::{FanMask, notify_opened_file_event},
    prelude::*,
    task::files::Fd,
};

#[syscall(SYS_FTRUNCATE)]
fn sys_ftruncate(fd: Fd, length: i64) -> Result<u64, SysError> {
    if length < 0 {
        return Err(SysError::InvalidArgument);
    }

    let task = get_current_task();
    let cred = task.cred();
    let file = task.get_fd(fd)?;
    file.truncate(length as u64, &cred)?;
    notify_opened_file_event(&file, FanMask::MODIFY);

    Ok(0)
}
