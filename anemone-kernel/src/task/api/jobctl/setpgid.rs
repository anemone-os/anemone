//! setpgid system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/setpgid.2.html

use crate::prelude::*;

#[syscall(SYS_SETPGID)]
fn sys_setpgid(pid: i32, pgid: i32) -> Result<u64, SysError> {
    if pgid < 0 {
        return Err(SysError::InvalidArgument);
    }
    if pid < 0 {
        return Err(SysError::NoSuchProcess);
    }

    let caller = get_current_task().get_thread_group();
    let target_tgid = if pid == 0 {
        caller.tgid()
    } else {
        Tid::new(pid as u32)
    };
    let target = get_thread_group(&target_tgid).ok_or(SysError::NoSuchProcess)?;
    let new_pgid = if pgid == 0 {
        target.tgid()
    } else {
        Tid::new(pgid as u32)
    };

    target.move_to_process_group_if(new_pgid, |ctx| {
        if ctx.target_tgid == ctx.target_sid {
            return Err(SysError::PermissionDenied);
        }

        if ctx.target_tgid != caller.tgid() {
            if !ctx.target_is_child_of(&caller) {
                return Err(SysError::NoSuchProcess);
            }
            if ctx.target_sid != caller.sid() {
                return Err(SysError::PermissionDenied);
            }
            if ctx.target_has_executed {
                return Err(SysError::AccessDenied);
            }
        }

        Ok(())
    })?;

    Ok(0)
}
