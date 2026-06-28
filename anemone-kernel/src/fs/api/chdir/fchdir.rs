use crate::{prelude::*, task::files::Fd};

use super::kernel_chdir;

#[syscall(SYS_FCHDIR)]
fn sys_fchdir(fd: Fd) -> Result<u64, SysError> {
    let task = get_current_task();
    let file_desc = task.get_fd(fd)?;
    let path = file_desc.vfs_file().path().clone();
    kernel_chdir(path)
}
