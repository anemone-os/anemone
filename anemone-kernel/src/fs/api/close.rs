use crate::{prelude::*, task::files::Fd};

#[syscall(SYS_CLOSE)]
fn sys_close(fd: Fd) -> Result<u64, SysError> {
    with_current_task(|task| {
        task.close_fd(fd)
            .map(|_fd| 0)
            .ok_or(KernelError::BadFileDescriptor.into())
    })
}
