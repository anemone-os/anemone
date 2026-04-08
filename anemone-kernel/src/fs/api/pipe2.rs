//! pipe2 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/pipe2.2.html

use crate::prelude::{dt::UserWritePtr, *};

#[syscall(SYS_PIPE2)]
fn sys_pipe2(pipefd: UserWritePtr<[isize; 2]>, flags: u32) -> Result<u64, SysError> {
    todo!()
}
