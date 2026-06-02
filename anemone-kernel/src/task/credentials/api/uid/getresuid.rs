//! getresuid system call.

use crate::{
    prelude::{
        user_access::{UserWritePtr, user_addr},
        *,
    },
    task::credentials::Uid,
};

/// Writes the current task's real, effective, and saved user IDs to user memory.
///
/// Permission check: none; a task may always inspect its own user IDs. The
/// syscall still validates that all output pointers are writable user pointers.
///
/// Reference: <https://man7.org/linux/man-pages/man2/getresuid.2.html>.
#[syscall(SYS_GETRESUID)]
fn sys_getresuid(
    #[validate_with(user_addr)] ruidp: VirtAddr,
    #[validate_with(user_addr)] euidp: VirtAddr,
    #[validate_with(user_addr)] suidp: VirtAddr,
) -> Result<u64, SysError> {
    let task = get_current_task();
    let cred = task.cred();
    let uspace = task.clone_uspace_handle();
    let mut usp = uspace.lock();
    {
        UserWritePtr::<Uid>::try_new(ruidp, &mut usp)?.write(cred.uid.real);
    }
    {
        UserWritePtr::<Uid>::try_new(euidp, &mut usp)?.write(cred.uid.effective);
    }
    {
        UserWritePtr::<Uid>::try_new(suidp, &mut usp)?.write(cred.uid.saved);
    }
    Ok(0)
}
