//! setresuid system call.

use crate::{
    prelude::*,
    task::credentials::{Uid, UserId},
};

use super::{super::id::UserTarget, update_caps_by_fsuid, update_caps_by_uid};

/// Changes the current task's real, effective, and/or saved user IDs.
///
/// Permission check: any requested ID that is not already one of the current
/// real/effective/saved user IDs requires `CAP_SETUID`. The filesystem user ID
/// follows the resulting effective user ID.
///
/// Reference: <https://man7.org/linux/man-pages/man2/setresuid.2.html>.
#[syscall(SYS_SETRESUID)]
fn sys_setresuid(
    ruid: UserTarget<Uid>,
    euid: UserTarget<Uid>,
    suid: UserTarget<Uid>,
) -> Result<u64, SysError> {
    let task = get_current_task();
    task.update_cred_with(|old| {
        let old_uid = old.uid;
        let old_fsuid = old.uid.fs;
        let raw_ruid = ruid;
        let raw_euid = euid;
        let raw_suid = suid;
        let ruid = ruid.specified();
        let euid = euid.specified();
        let suid = suid.specified();
        let ruid_new = ruid.is_some_and(|ruid| !old.uid.matches_any_res(ruid));
        let euid_new = euid.is_some_and(|euid| !old.uid.matches_any_res(euid));
        let suid_new = suid.is_some_and(|suid| !old.uid.matches_any_res(suid));
        if (ruid_new || euid_new || suid_new) && !old.has_cap_effective(Capability::SETUID) {
            return Err(deny_permission!(
                "setresuid denied: requested=({},{},{}), uid={}, euid={}, suid={}, missing={:?}",
                raw_ruid,
                raw_euid,
                raw_suid,
                old.uid.real.get(),
                old.uid.effective.get(),
                old.uid.saved.get(),
                Capability::SETUID
            ));
        }
        let resulting_euid = euid.unwrap_or(old_uid.effective);
        if ruid.map_or(true, |ruid| ruid == old.uid.real)
            && euid.map_or(true, |euid| euid == old.uid.effective)
            && suid.map_or(true, |suid| suid == old.uid.saved)
            && old.uid.fs == resulting_euid
        {
            return Ok(());
        }
        if let Some(ruid) = ruid {
            old.uid.real = ruid;
        }
        if let Some(euid) = euid {
            old.uid.effective = euid;
        }
        if let Some(suid) = suid {
            old.uid.saved = suid;
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
