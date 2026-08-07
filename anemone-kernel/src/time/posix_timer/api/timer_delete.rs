//! `timer_delete` system call.

use crate::prelude::*;

#[syscall(SYS_TIMER_DELETE)]
fn sys_timer_delete(timer_id: i32) -> Result<u64, SysError> {
    get_current_task()
        .get_thread_group()
        .delete_posix_timer(timer_id)?;
    Ok(0)
}
