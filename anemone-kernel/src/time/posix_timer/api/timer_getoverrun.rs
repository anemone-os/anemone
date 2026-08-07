//! `timer_getoverrun` system call.

use crate::prelude::*;

#[syscall(SYS_TIMER_GETOVERRUN)]
fn sys_timer_getoverrun(timer_id: i32) -> Result<u64, SysError> {
    let overrun = get_current_task()
        .get_thread_group()
        .posix_timer_getoverrun(timer_id)?;
    Ok(overrun as u64)
}
