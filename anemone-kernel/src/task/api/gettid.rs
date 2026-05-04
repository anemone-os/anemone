//! gettid system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/gettid.2.html

use crate::prelude::*;

#[syscall(SYS_GETTID)]
pub fn sys_gettid() -> Result<u64, SysError> {
    Ok(get_current_task().tid().get() as u64)
}
