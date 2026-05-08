//! setgid system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/setgid.2.html

use crate::prelude::*;

#[syscall(SYS_SETGID)]
fn sys_setgid(gid: u32) -> Result<u64, SysError> {
    kdebugln!("setgid: gid={}", gid);

    Ok(0)
}
