//! dup system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/dup3.2.html

use crate::{prelude::*, task::files::Fd};

#[syscall(SYS_DUP)]
fn sys_dup(oldfd: Fd) -> Result<u64, SysError> {
    let task = get_current_task();
    task.dup(oldfd).map(|newfd| newfd.raw() as u64)
}
