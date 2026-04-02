use crate::prelude::*;

#[syscall(SYS_CLOSE)]
fn sys_close(fd: usize) -> Result<u64, SysError> {
    with_current_task(|task| {
        task.close_fd(fd)
            .map(|_| 0)
            .ok_or(KernelError::BadFileDescriptor.into())
    })
}
