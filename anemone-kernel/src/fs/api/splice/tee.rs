//! tee system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/tee.2.html

use crate::{
    fs::pipe::{PipeEndpointSide, pipe_endpoints_same_pipe},
    prelude::*,
};

use super::{SpliceFlags, parse_fd, pipe_endpoint_of};

#[syscall(SYS_TEE)]
fn sys_tee(raw_fd_in: u64, raw_fd_out: u64, len: usize, raw_flags: u64) -> Result<u64, SysError> {
    kdebugln!(
        "sys_tee: fd_in={:#x}, fd_out={:#x}, len={}, flags={:#x}",
        raw_fd_in,
        raw_fd_out,
        len,
        raw_flags
    );

    let flags = SpliceFlags::parse(raw_flags)?;
    if len == 0 {
        return Ok(0);
    }

    let fd_in = parse_fd(raw_fd_in)?;
    let fd_out = parse_fd(raw_fd_out)?;
    let task = get_current_task();
    let in_fd = task.get_fd(fd_in)?;
    let out_fd = task.get_fd(fd_out)?;

    let Some(in_pipe) = pipe_endpoint_of(&in_fd) else {
        return Err(SysError::InvalidArgument);
    };
    if in_pipe.side() != PipeEndpointSide::Read {
        return Err(SysError::InvalidArgument);
    }

    let Some(out_pipe) = pipe_endpoint_of(&out_fd) else {
        return Err(SysError::InvalidArgument);
    };
    if out_pipe.side() != PipeEndpointSide::Write {
        return Err(SysError::InvalidArgument);
    }

    if pipe_endpoints_same_pipe(in_fd.vfs_file().as_ref(), out_fd.vfs_file().as_ref())? {
        return Err(SysError::InvalidArgument);
    }

    flags.reject_nonblock_functional_path("sys_tee")?;

    // A consume-and-copy fallback would violate tee(2)'s external contract:
    // input pipe buffers must be duplicated, not consumed. Keep the valid
    // pipe-to-pipe path visibly unsupported until pipe-buffer duplication is
    // introduced by a later stage.
    knoticeln!("sys_tee: pipe buffer duplication is not supported yet");
    Err(SysError::NotSupported)
}
