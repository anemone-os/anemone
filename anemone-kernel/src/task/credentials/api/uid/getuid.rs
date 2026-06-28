//! getuid system call.

use crate::prelude::*;

/// Returns the current task's real user ID.
///
/// Permission check: none; a task may always inspect its own real user ID.
///
/// Reference: <https://man7.org/linux/man-pages/man2/getuid.2.html>.
#[syscall(SYS_GETUID)]
fn sys_getuid() -> Result<u64, SysError> {
    kdebugln!("getuid");

    Ok(get_current_task().cred().uid.real.get() as u64)
}
