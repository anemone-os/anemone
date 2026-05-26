//! setpgid system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/setpgid.2.html

use crate::prelude::*;

#[syscall(SYS_SETPGID)]
fn sys_setpgid(tgid: Tid, pgid: Tid) -> Result<u64, SysError> {
    todo!()
}
