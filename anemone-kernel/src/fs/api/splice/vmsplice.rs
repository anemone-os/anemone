//! vmsplice system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/vmsplice.2.html

use anemone_abi::fs::linux::IOV_MAX;

use crate::prelude::*;

use super::{SpliceFlags, parse_fd, pipe_endpoint_of};

#[syscall(SYS_VMSPLICE)]
fn sys_vmsplice(
    raw_fd: u64,
    raw_iov: u64,
    nr_segs: usize,
    raw_flags: u64,
) -> Result<u64, SysError> {
    kdebugln!(
        "sys_vmsplice: fd={:#x}, iov={:#x}, nr_segs={}, flags={:#x}",
        raw_fd,
        raw_iov,
        nr_segs,
        raw_flags
    );

    let flags = SpliceFlags::parse(raw_flags)?;
    let fd = parse_fd(raw_fd)?;
    let task = get_current_task();
    let file = task.get_fd(fd)?;

    if pipe_endpoint_of(&file).is_none() {
        return Err(SysError::BadFileDescriptor);
    }

    if nr_segs > IOV_MAX {
        return Err(SysError::InvalidArgument);
    }

    flags.reject_nonblock_functional_path("sys_vmsplice")?;

    // This stage intentionally avoids importing user iovecs or pinning/copying
    // pages into pipe buffers. That keeps vmsplice02 errno coverage separate
    // from the larger pipe-capacity, full-pipe, and SPLICE_F_GIFT semantics.
    let _ = raw_iov;
    knoticeln!("sys_vmsplice: user iovec to/from pipe transfer is not supported yet");
    Err(SysError::NotSupported)
}
