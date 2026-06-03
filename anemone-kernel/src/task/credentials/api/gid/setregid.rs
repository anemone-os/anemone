//! setregid system call.

use crate::{prelude::*, task::credentials::Gid};

use super::super::id::UserTarget;

/// Changes the current task's real and/or effective group ID.
///
/// Permission check: each specified ID may be set without `CAP_SETGID` only
/// when it matches the current real or effective group ID for `rgid`, or any of
/// real/effective/saved for `egid`; otherwise the caller must have
/// `CAP_SETGID`.
///
/// Reference: <https://man7.org/linux/man-pages/man2/setregid.2.html>.
#[syscall(SYS_SETREGID)]
fn sys_setregid(rgid: UserTarget<Gid>, egid: UserTarget<Gid>) -> Result<u64, SysError> {
    let task = get_current_task();
    task.update_cred_with(|old| {
        let old_gid = old.gid;
        let rgid = rgid.specified();
        let egid = egid.specified();
        if let Some(rgid) = rgid {
            if rgid != old.gid.real
                && rgid != old.gid.effective
                && !old.has_cap_effective(Capability::SETGID)
            {
                return Err(deny_permission!(
                    "setregid denied: requested rgid={}, gid={}, egid={}, missing={:?}",
                    rgid.get(),
                    old.gid.real.get(),
                    old.gid.effective.get(),
                    Capability::SETGID
                ));
            }
        }
        if let Some(egid) = egid {
            if !old.gid.matches_any_res(egid) && !old.has_cap_effective(Capability::SETGID) {
                return Err(deny_permission!(
                    "setregid denied: requested egid={}, gid={}, egid={}, sgid={}, missing={:?}",
                    egid.get(),
                    old.gid.real.get(),
                    old.gid.effective.get(),
                    old.gid.saved.get(),
                    Capability::SETGID
                ));
            }
        }
        let updates_saved = rgid.is_some() || egid.is_some_and(|egid| egid != old_gid.real);
        if let Some(rgid) = rgid {
            old.gid.real = rgid;
        }
        if let Some(egid) = egid {
            old.gid.effective = egid;
        }
        if updates_saved {
            old.gid.saved = old.gid.effective;
        }
        old.gid.fs = old.gid.effective;
        Ok(())
    })?;
    Ok(0)
}
