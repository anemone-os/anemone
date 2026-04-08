//! umount system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/umount.2.html

use crate::prelude::{dt::c_readonly_string, *};

#[syscall(SYS_UMOUNT2)]
fn sys_umount(
    #[validate_with(c_readonly_string)] target: Box<str>,
    flags: u64,
) -> Result<u64, SysError> {
    // todo
    Ok(0)
}
