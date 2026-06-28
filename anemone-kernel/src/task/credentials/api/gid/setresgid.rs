//! setresgid system call.

use crate::{prelude::*, task::credentials::Gid};

use super::super::id::UserTarget;

/// Changes the current task's real, effective, and/or saved group IDs.
///
/// Permission check: any requested ID that is not already one of the current
/// real/effective/saved group IDs requires `CAP_SETGID`. The filesystem group
/// ID follows the resulting effective group ID.
///
/// Reference: <https://man7.org/linux/man-pages/man2/setresgid.2.html>.
#[syscall(SYS_SETRESGID)]
fn sys_setresgid(
    rgid: UserTarget<Gid>,
    egid: UserTarget<Gid>,
    sgid: UserTarget<Gid>,
) -> Result<u64, SysError> {
    let task = get_current_task();
    task.update_cred_with(|old| {
        let old_gid = old.gid;
        let raw_rgid = rgid;
        let raw_egid = egid;
        let raw_sgid = sgid;
        let rgid = rgid.specified();
        let egid = egid.specified();
        let sgid = sgid.specified();
        let rgid_new = rgid.is_some_and(|rgid| !old.gid.matches_any_res(rgid));
        let egid_new = egid.is_some_and(|egid| !old.gid.matches_any_res(egid));
        let sgid_new = sgid.is_some_and(|sgid| !old.gid.matches_any_res(sgid));
        if (rgid_new || egid_new || sgid_new) && !old.has_cap_effective(Capability::SETGID) {
            return Err(deny_permission!(
                "setresgid denied: requested=({},{},{}), gid={}, egid={}, sgid={}, missing={:?}",
                raw_rgid,
                raw_egid,
                raw_sgid,
                old.gid.real.get(),
                old.gid.effective.get(),
                old.gid.saved.get(),
                Capability::SETGID
            ));
        }
        let resulting_egid = egid.unwrap_or(old_gid.effective);
        if rgid.map_or(true, |rgid| rgid == old.gid.real)
            && egid.map_or(true, |egid| egid == old.gid.effective)
            && sgid.map_or(true, |sgid| sgid == old.gid.saved)
            && old.gid.fs == resulting_egid
        {
            return Ok(());
        }
        if let Some(rgid) = rgid {
            old.gid.real = rgid;
        }
        if let Some(egid) = egid {
            old.gid.effective = egid;
        }
        if let Some(sgid) = sgid {
            old.gid.saved = sgid;
        }
        old.gid.fs = old.gid.effective;
        Ok(())
    })?;
    Ok(0)
}
