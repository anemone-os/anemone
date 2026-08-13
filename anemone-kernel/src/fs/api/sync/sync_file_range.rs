//! sync_file_range system call.

use anemone_abi::fs::linux::sync_file_range::SYNC_FILE_RANGE_VALID_FLAGS;

use crate::{
    prelude::{handler::syscall_arg_flag32, *},
    task::files::Fd,
};

use super::accepts_sync_file_range_stub;

#[syscall(SYS_SYNC_FILE_RANGE)]
fn sys_sync_file_range(
    fd: Fd,
    offset: i64,
    nbytes: i64,
    raw_flags: u64,
) -> Result<u64, SysError> {
    let task = get_current_task();
    let file = task.get_fd(fd)?;

    let flags = syscall_arg_flag32(raw_flags)?;
    if flags & !SYNC_FILE_RANGE_VALID_FLAGS != 0
        || offset < 0
        || nbytes < 0
        || offset.checked_add(nbytes).is_none()
    {
        return Err(SysError::InvalidArgument);
    }

    if !accepts_sync_file_range_stub(&file) {
        return Err(SysError::IllegalSeek);
    }

    // This is deliberately a success-no-op compatibility stub: it validates
    // the Linux ABI but does not start or wait for range writeback. Replace it
    // when the VFS exposes range-owned writeback and error reporting.
    kdebugln!(
        "sync_file_range is not implemented; returning success for fd {:?}, offset={}, nbytes={}, flags={:#x}",
        fd,
        offset,
        nbytes,
        flags
    );
    Ok(0)
}
