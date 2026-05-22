//! setuid system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/setuid.2.html

use crate::prelude::*;

#[syscall(SYS_SETUID)]
fn sys_setuid(uid: u32) -> Result<u64, SysError> {
    kdebugln!("setuid: uid={}", uid);

    Ok(0)
}
