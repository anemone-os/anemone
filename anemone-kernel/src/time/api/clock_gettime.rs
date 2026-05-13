//! clock_gettime system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/clock_gettime.2.html

use crate::{prelude::*, syscall::user_access::user_addr};

#[syscall(SYS_CLOCK_GETTIME)]
fn sys_clock_gettime(
    which_clock: u32,
    #[validate_with(user_addr)] tp: VirtAddr,
) -> Result<u64, SysError> {
    kdebugln!("clock_gettime: which_clock={:#x}, tp={:?}", which_clock, tp);

    Err(SysError::NotYetImplemented)
}
