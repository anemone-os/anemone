use crate::{prelude::*, task::files::Fd};

#[syscall(SYS_CLOSE)]
fn sys_close(fd: Fd) -> Result<u64, SysError> {
    let task = get_current_task();
    task.close_fd(fd).map(|_fd| 0)
}
