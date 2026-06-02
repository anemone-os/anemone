//! getgid system call.

use crate::{prelude::*, task::credentials::UserId};

/// Returns the current task's real group ID.
///
/// Permission check: none; a task may always inspect its own real group ID.
///
/// Reference: <https://man7.org/linux/man-pages/man2/getgid.2.html>.
#[syscall(SYS_GETGID)]
fn sys_getgid() -> Result<u64, SysError> {
    kdebugln!("getgid");

    Ok(get_current_task().cred().gid.real.get() as u64)
}
