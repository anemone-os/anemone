//! setuid system call.

use crate::{
    prelude::*,
    task::credentials::{Uid, UserId},
};

use super::{update_caps_by_fsuid, update_caps_by_uid};

/// Sets the current task's user IDs.
///
/// Permission check: with `CAP_SETUID`, real/effective/saved/filesystem user
/// IDs are all set to the requested ID. Without `CAP_SETUID`, the requested ID
/// must match the current real or saved user ID, and only the effective and
/// filesystem user IDs are changed.
///
/// Reference: <https://man7.org/linux/man-pages/man2/setuid.2.html>.
#[syscall(SYS_SETUID)]
fn sys_setuid(uid: Uid) -> Result<u64, SysError> {
    kdebugln!("setuid: uid={}", uid);

    let task = get_current_task();
    task.update_cred_with(|old| {
        let old_uid = old.uid;
        let old_fsuid = old.uid.fs;
        if old.has_cap_effective(Capability::SETUID) {
            old.uid.real = uid;
            old.uid.effective = uid;
            old.uid.saved = uid;
            old.uid.fs = uid;
        } else if uid == old.uid.real || uid == old.uid.saved {
            old.uid.effective = uid;
            old.uid.fs = uid;
        } else {
            return Err(deny_permission!(
                "setuid denied: requested={}, uid={}, euid={}, suid={}, missing={:?}",
                uid.get(),
                old.uid.real.get(),
                old.uid.effective.get(),
                old.uid.saved.get(),
                Capability::SETUID
            ));
        }
        update_caps_by_uid(old, old_uid);
        if old.uid.fs != old_fsuid {
            update_caps_by_fsuid(old, old_fsuid);
        }
        Ok(())
    })?;
    Ok(0)
}
