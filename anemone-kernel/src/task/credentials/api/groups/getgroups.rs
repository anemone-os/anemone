//! getgroups system call.

use crate::{
    prelude::{
        user_access::{UserWritePtr, user_addr},
        *,
    },
    task::credentials::Gid,
};

/// Returns the current task's supplementary group IDs.
///
/// Permission check: none; a task may always inspect its own supplementary
/// groups. If `gidsetsize` is zero, only the number of groups is returned;
/// otherwise the output buffer must be large enough and writable.
///
/// Reference: <https://man7.org/linux/man-pages/man2/getgroups.2.html>.
#[syscall(SYS_GETGROUPS)]
fn sys_getgroups(gidsetsize: i32, grouplist: u64) -> Result<u64, SysError> {
    kdebugln!(
        "getgroups: gidsetsize={}, grouplist={:#x}",
        gidsetsize,
        grouplist
    );

    if gidsetsize < 0 {
        return Err(SysError::InvalidArgument);
    }

    let cred = get_current_task().cred();
    let groups = cred.groups.as_slice();
    if gidsetsize == 0 {
        return Ok(groups.len() as u64);
    }
    let gidsetsize = gidsetsize as usize;
    if groups.len() > gidsetsize {
        return Err(SysError::InvalidArgument);
    }
    if groups.is_empty() {
        return Ok(0);
    }

    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut usp = uspace.lock();
    {
        let mut list = UserWritePtr::<[Gid]>::try_new(
            user_addr(grouplist).map_err(|_| SysError::BadAddress)?,
            groups.len(),
            &mut usp,
        )?;
        list.copy_from_slice(groups);
    }
    Ok(groups.len() as u64)
}
