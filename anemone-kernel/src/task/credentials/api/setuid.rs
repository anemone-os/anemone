//! setuid system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/setuid.2.html

use crate::{prelude::*, task::credentials::Uid};

#[syscall(SYS_SETUID)]
fn sys_setuid(uid: Uid) -> Result<u64, SysError> {
    kdebugln!("setuid: uid={}", uid);

    Ok(0)
}
