//! setgroups system call.

use crate::{
    prelude::{
        user_access::{UserReadPtr, user_addr},
        *,
    },
    task::credentials::{
        Gid,
        cap::Capability,
        groups::NGROUPS_MAX,
    },
};

/// Replaces the current task's supplementary group list.
///
/// Permission check: the caller must have `CAP_SETGID`. The requested list size
/// must be non-negative and no larger than `NGROUPS_MAX`; the group list is
/// copied from user memory, sorted, and deduplicated before being installed.
///
/// Reference: <https://man7.org/linux/man-pages/man2/setgroups.2.html>.
#[syscall(SYS_SETGROUPS)]
fn sys_setgroups(gidsetsize: i32, grouplist: u64) -> Result<u64, SysError> {
    kdebugln!(
        "setgroups: gidsetsize={}, grouplist={:?}",
        gidsetsize,
        grouplist
    );

    let task = get_current_task();
    if !task.has_cap(Capability::SETGID) {
        return Err(deny_permission!(
            "setgroups denied: missing={:?}",
            Capability::SETGID
        ));
    }

    if gidsetsize < 0 {
        return Err(SysError::InvalidArgument);
    }
    let gidsetsize = usize::try_from(gidsetsize).map_err(|_| SysError::InvalidArgument)?;
    if gidsetsize > NGROUPS_MAX {
        return Err(SysError::InvalidArgument);
    }

    let mut groups = if gidsetsize == 0 {
        Vec::new()
    } else {
        let uspace = task.clone_uspace_handle();
        let mut usp = uspace.lock();
        let mut groups = vec![Gid::ROOT; gidsetsize];
        {
            let list = UserReadPtr::<[Gid]>::try_new(
                user_addr(grouplist).map_err(|_| SysError::BadAddress)?,
                gidsetsize,
                &mut usp,
            )?;
            list.copy_to_slice(&mut groups);
        }
        groups
    };
    groups.sort();
    groups.dedup();

    task.update_cred_with(|old| {
        old.groups = groups;
        Ok(())
    })?;
    Ok(0)
}
