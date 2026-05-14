//! ioctl system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/ioctl.2.html

use crate::{prelude::*, task::files::Fd};

#[syscall(SYS_IOCTL)]
fn sys_ioctl(fd: Fd, cmd: u32, arg: u64) -> Result<u64, SysError> {
    kdebugln!(
        "[NYI]sys_ioctl: fd={:?}, cmd={:#x}, arg={:#x}",
        fd,
        cmd,
        arg
    );

    Err(SysError::NotYetImplemented)
}
