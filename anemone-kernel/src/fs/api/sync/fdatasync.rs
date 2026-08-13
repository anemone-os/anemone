//! fdatasync system call.

use crate::{prelude::*, task::files::Fd};

use super::accepts_fsync_stub;

#[syscall(SYS_FDATASYNC)]
fn sys_fdatasync(fd: Fd) -> Result<u64, SysError> {
    let task = get_current_task();
    let file = task.get_fd(fd)?;
    if !accepts_fsync_stub(&file) {
        return Err(SysError::InvalidArgument);
    }

    // This matches the current fsync compatibility stub: validate the descriptor
    // but defer data-only writeback until the VFS exposes fdatasync semantics.
    kdebugln!(
        "fdatasync is not implemented; returning success for fd {:?}",
        fd
    );
    Ok(0)
}
