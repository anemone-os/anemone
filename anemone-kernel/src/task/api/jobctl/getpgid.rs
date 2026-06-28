//! getpgid system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/getpgid.2.html

use crate::prelude::*;

#[syscall(SYS_GETPGID)]
fn sys_getpgid(pid: i32) -> Result<u64, SysError> {
    kdebugln!("getpgid: pid={}", pid);

    if pid < 0 {
        return Err(SysError::NoSuchProcess);
    }

    let tg = if pid == 0 {
        get_current_task().get_thread_group()
    } else {
        let tg = get_thread_group(&Tid::new(pid as u32)).ok_or(SysError::NoSuchProcess)?;
        if tg.ty() != ThreadGroupType::User {
            return Err(SysError::NoSuchProcess);
        }
        tg
    };

    Ok(tg.pgid().get() as u64)
}
