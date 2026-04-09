//! dup3 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/dup3.2.html

use crate::{prelude::*, task::files::FdFlags};

#[syscall(SYS_DUP3)]
fn sys_dup3(oldfd: usize, newfd: usize, flags: u32) -> Result<u64, SysError> {
    with_current_task(|task| {
        task.dup3(oldfd, newfd, FdFlags::from_dup3_flags(flags)?)
            .map(|newfd| newfd as u64)
    })
}
