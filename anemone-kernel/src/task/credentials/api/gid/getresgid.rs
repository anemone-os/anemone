//! getresgid system call.

use crate::{
    prelude::{
        user_access::{UserWritePtr, user_addr},
        *,
    },
    task::credentials::Gid,
};

/// Writes the current task's real, effective, and saved group IDs to user memory.
///
/// Permission check: none; a task may always inspect its own group IDs. The
/// syscall still validates that all output pointers are writable user pointers.
///
/// Reference: <https://man7.org/linux/man-pages/man2/getresgid.2.html>.
#[syscall(SYS_GETRESGID)]
fn sys_getresgid(
    #[validate_with(user_addr)] rgidp: VirtAddr,
    #[validate_with(user_addr)] egidp: VirtAddr,
    #[validate_with(user_addr)] sgidp: VirtAddr,
) -> Result<u64, SysError> {
    let task = get_current_task();
    let cred = task.cred();
    let uspace = task.clone_uspace_handle();
    let mut usp = uspace.lock();
    {
        UserWritePtr::<Gid>::try_new(rgidp, &mut usp)?.write(cred.gid.real);
    }
    {
        UserWritePtr::<Gid>::try_new(egidp, &mut usp)?.write(cred.gid.effective);
    }
    {
        UserWritePtr::<Gid>::try_new(sgidp, &mut usp)?.write(cred.gid.saved);
    }
    Ok(0)
}
