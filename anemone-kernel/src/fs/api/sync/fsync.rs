//! fsync system call.

use crate::{prelude::*, task::files::Fd};

#[syscall(SYS_FSYNC)]
fn sys_fsync(fd: Fd) -> Result<u64, SysError> {
    let task = get_current_task();
    task.get_fd(fd)?;

    Ok(0)
}
