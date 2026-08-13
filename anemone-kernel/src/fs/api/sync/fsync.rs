//! fsync system call.

use crate::{prelude::*, task::files::Fd};

use super::accepts_fsync_stub;

#[syscall(SYS_FSYNC)]
fn sys_fsync(fd: Fd) -> Result<u64, SysError> {
    let task = get_current_task();
    let file = task.get_fd(fd)?;
    if !accepts_fsync_stub(&file) {
        return Err(SysError::InvalidArgument);
    }

    // Preserve the current success-no-op ABI until VFS exposes a per-file sync
    // operation. The validation above must remain even while writeback is absent.
    kdebugln!("fsync is not implemented; returning success for fd {:?}", fd);
    Ok(0)
}
