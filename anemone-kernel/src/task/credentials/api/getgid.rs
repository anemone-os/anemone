//! getgid system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/getgid.2.html

use crate::prelude::*;

#[syscall(SYS_GETGID)]
fn sys_getgid() -> Result<u64, SysError> {
    kdebugln!("getgid");

    Ok(0)
}
