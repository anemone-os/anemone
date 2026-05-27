//! getsid system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/getsid.2.html

use crate::prelude::*;

#[syscall(SYS_GETSID)]
fn sys_getsid(pid: i32) -> Result<u64, SysError> {
    if pid < 0 {
        return Err(SysError::NoSuchProcess);
    }

    let tgid = if pid == 0 {
        get_current_task().tgid()
    } else {
        Tid::new(pid as u32)
    };
    let tg = get_thread_group(&tgid).ok_or(SysError::NoSuchProcess)?;

    Ok(tg.sid().get() as u64)
}
