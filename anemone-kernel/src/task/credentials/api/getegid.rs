//! getegid system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/getegid.2.html

use crate::prelude::*;

#[syscall(SYS_GETEGID)]
fn sys_getegid() -> Result<u64, SysError> {
    kdebugln!("getegid");

    Ok(0)
}
