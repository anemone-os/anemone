//! geteuid system call.

use crate::prelude::*;

/// Returns the current task's effective user ID.
///
/// Permission check: none; a task may always inspect its own effective user ID.
///
/// Reference: <https://man7.org/linux/man-pages/man2/geteuid.2.html>.
#[syscall(SYS_GETEUID)]
fn sys_geteuid() -> Result<u64, SysError> {
    kdebugln!("geteuid");

    Ok(get_current_task().cred().uid.effective.get() as u64)
}
