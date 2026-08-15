use crate::{prelude::*, syscall::user_access::user_addr, task::files::Fd};

use super::write_statfs;

#[syscall(SYS_FSTATFS)]
fn sys_fstatfs(fd: Fd, #[validate_with(user_addr)] buf: VirtAddr) -> Result<u64, SysError> {
    kdebugln!("sys_fstatfs: fd={fd:?}, buf={buf:?}");

    let task = get_current_task();
    let file_desc = task.get_fd(fd)?;

    // Linux fd_statfs() uses fdget_raw(): fstatfs is a path query and accepts
    // O_PATH descriptions without requiring read or write access.
    write_statfs(file_desc.vfs_file().path().mount(), buf)?;

    Ok(0)
}
