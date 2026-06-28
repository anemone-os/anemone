//! setreuid system call.

use crate::{prelude::*, task::credentials::Uid};

use super::{super::id::UserTarget, update_caps_by_fsuid, update_caps_by_uid};

/// Changes the current task's real and/or effective user ID.
///
/// Permission check: each specified ID may be set without `CAP_SETUID` only
/// when it matches the current real or effective user ID for `ruid`, or any of
/// real/effective/saved for `euid`; otherwise the caller must have
/// `CAP_SETUID`.
///
/// Reference: <https://man7.org/linux/man-pages/man2/setreuid.2.html>.
#[syscall(SYS_SETREUID)]
fn sys_setreuid(ruid: UserTarget<Uid>, euid: UserTarget<Uid>) -> Result<u64, SysError> {
    let task = get_current_task();
    task.update_cred_with(|old| {
        let old_uid = old.uid;
        let old_fsuid = old.uid.fs;
        let ruid = ruid.specified();
        let euid = euid.specified();
        if let Some(ruid) = ruid {
            if ruid != old.uid.real
                && ruid != old.uid.effective
                && !old.has_cap_effective(Capability::SETUID)
            {
                return Err(deny_permission!(
                    "setreuid denied: requested ruid={}, uid={}, euid={}, missing={:?}",
                    ruid.get(),
                    old.uid.real.get(),
                    old.uid.effective.get(),
                    Capability::SETUID
                ));
            }
        }
        if let Some(euid) = euid {
            if !old.uid.matches_any_res(euid) && !old.has_cap_effective(Capability::SETUID) {
                return Err(deny_permission!(
                    "setreuid denied: requested euid={}, uid={}, euid={}, suid={}, missing={:?}",
                    euid.get(),
                    old.uid.real.get(),
                    old.uid.effective.get(),
                    old.uid.saved.get(),
                    Capability::SETUID
                ));
            }
        }
        let updates_saved = ruid.is_some() || euid.is_some_and(|euid| euid != old_uid.real);
        if let Some(ruid) = ruid {
            old.uid.real = ruid;
        }
        if let Some(euid) = euid {
            old.uid.effective = euid;
        }
        if updates_saved {
            old.uid.saved = old.uid.effective;
        }
        old.uid.fs = old.uid.effective;
        update_caps_by_uid(old, old_uid);
        if old.uid.fs != old_fsuid {
            update_caps_by_fsuid(old, old_fsuid);
        }
        Ok(())
    })?;
    Ok(0)
}
