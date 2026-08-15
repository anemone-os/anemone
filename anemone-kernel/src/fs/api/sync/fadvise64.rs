//! fadvise64 system call.

use anemone_abi::fs::linux::fadvise::*;

use crate::{prelude::*, task::files::Fd};

fn is_valid_advice(advice: i32) -> bool {
    matches!(
        advice,
        POSIX_FADV_NORMAL
            | POSIX_FADV_RANDOM
            | POSIX_FADV_SEQUENTIAL
            | POSIX_FADV_WILLNEED
            | POSIX_FADV_DONTNEED
            | POSIX_FADV_NOREUSE
    )
}

#[syscall(SYS_FADVISE64)]
fn sys_fadvise64(fd: Fd, offset: i64, len: i64, advice: i32) -> Result<u64, SysError> {
    let task = get_current_task();
    let file = task.get_fd(fd)?;

    // Linux checks FIFO before range and advice validity, so ESPIPE wins over
    // EINVAL when a pipe is combined with an otherwise invalid request.
    if file.vfs_file().inode().ty() == InodeType::Fifo {
        return Err(SysError::IllegalSeek);
    }

    if len < 0 || !is_valid_advice(advice) {
        return Err(SysError::InvalidArgument);
    }

    // This is deliberately a success-no-op compatibility stub: valid advice
    // does not alter readahead, writeback, or page-cache state. Replace this
    // path when the VFS exposes an address-space-owned fadvise handoff.
    kdebugln!(
        "fadvise64 is not implemented; returning success for fd {:?}, offset={}, len={}, advice={}",
        fd,
        offset,
        len,
        advice
    );
    Ok(0)
}
