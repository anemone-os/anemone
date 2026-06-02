//! getegid system call.

use crate::{prelude::*, task::credentials::UserId};

/// Returns the current task's effective group ID.
///
/// Permission check: none; a task may always inspect its own effective group ID.
///
/// Reference: <https://man7.org/linux/man-pages/man2/getegid.2.html>.
#[syscall(SYS_GETEGID)]
fn sys_getegid() -> Result<u64, SysError> {
    kdebugln!("getegid");

    Ok(get_current_task().cred().gid.effective.get() as u64)
}
