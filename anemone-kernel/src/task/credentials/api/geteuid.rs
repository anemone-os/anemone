//! geteuid system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/geteuid.2.html

use crate::prelude::*;

#[syscall(SYS_GETEUID)]
fn sys_geteuid() -> Result<u64, SysError> {
    kdebugln!("geteuid");

    Ok(0)
}
