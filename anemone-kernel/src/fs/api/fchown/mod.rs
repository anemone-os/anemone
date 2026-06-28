//! fchown / fchownat system calls.
//!
//! References:
//! - https://www.man7.org/linux/man-pages/man2/chown.2.html
//! - `etc/linux-6.6.32/fs/open.c`

pub mod fchown;
pub mod fchownat;

use crate::prelude::*;

pub(super) fn owner_from_syscall(owner: Uid) -> Option<Uid> {
    // Linux uses (uid_t)-1 as a per-argument no-change sentinel.
    (owner.get() != u32::MAX).then_some(owner)
}

pub(super) fn group_from_syscall(group: Gid) -> Option<Gid> {
    // Linux uses (gid_t)-1 as a per-argument no-change sentinel.
    (group.get() != u32::MAX).then_some(group)
}

pub fn kernel_fchown(
    pathref: &PathRef,
    owner: Option<Uid>,
    group: Option<Gid>,
    ctime: Duration,
) -> Result<(), SysError> {
    let checker = FsPermChecker::for_current_fs();
    let inode = pathref.inode();

    pathref.mount().ensure_writable()?;

    if let Some(owner) = owner {
        if !(checker.is_owner(inode) && owner == inode.uid()) && !checker.has_cap(Capability::CHOWN)
        {
            return Err(SysError::PermissionDenied);
        }
    }

    if let Some(group) = group {
        if !(checker.is_owner(inode) && (group == inode.gid() || checker.fs_group_allowed(group)))
            && !checker.has_cap(Capability::CHOWN)
        {
            return Err(SysError::PermissionDenied);
        }
    }

    let cred = get_current_task().cred();
    inode.chown(owner, group, ctime);
    inode.after_modified(&cred, ModifType::Own, ctime);
    Ok(())
}
