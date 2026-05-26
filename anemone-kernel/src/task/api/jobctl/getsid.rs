//! getsid system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/getsid.2.html

use crate::prelude::*;

#[syscall(SYS_GETSID)]
fn sys_getsid(tgid: Tid) -> Result<u64, SysError> {
    todo!()
}
