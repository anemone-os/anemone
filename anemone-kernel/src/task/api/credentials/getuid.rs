//! getuid system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/getuid.2.html

use crate::prelude::*;

#[syscall(SYS_GETUID)]
fn sys_getuid() -> Result<u64, SysError> {
    kdebugln!("getuid");

    Ok(0)
}
