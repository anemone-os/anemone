//! setsid system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/setsid.2.html

use crate::prelude::*;

#[syscall(SYS_SETSID)]
fn sys_setsid() -> Result<u64, SysError> {
    kdebugln!("setsid");

    let sid = get_current_task().get_thread_group().create_session()?;
    Ok(sid.get() as u64)
}
