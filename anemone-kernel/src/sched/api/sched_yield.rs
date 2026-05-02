//! sched_yield system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/sched_yield.2.html

use crate::prelude::*;

#[syscall(SYS_SCHED_YIELD)]
fn sys_yield() -> Result<u64, SysError> {
    yield_now();
    Ok(0)
}
