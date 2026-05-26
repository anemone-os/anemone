//! getpgid system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/getpgid.2.html

use crate::prelude::*;

#[syscall(SYS_GETPGID)]
fn sys_getpgid(tgid: Tid) -> Result<u64, SysError> {
    todo!()
}
