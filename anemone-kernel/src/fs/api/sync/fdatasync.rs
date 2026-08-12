//! fdatasync system call.

use crate::{prelude::*, task::files::Fd};

#[syscall(SYS_FDATASYNC)]
fn sys_fdatasync(fd: Fd) -> Result<u64, SysError> {
    let task = get_current_task();
    task.get_fd(fd)?;

    // This matches the current fsync compatibility stub: validate the descriptor
    // but defer data-only writeback until the VFS exposes fdatasync semantics.
    kdebugln!(
        "fdatasync is not implemented; returning success for fd {:?}",
        fd
    );
    Ok(0)
}
