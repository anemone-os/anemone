//! dup system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/dup3.2.html

use crate::prelude::*;

#[syscall(SYS_DUP)]
fn sys_dup(oldfd: usize) -> Result<u64, SysError> {
    with_current_task(|task| {
        task.dup(oldfd)
            .map(|newfd| newfd as u64)
            .ok_or(KernelError::BadFileDescriptor.into())
    })
}
