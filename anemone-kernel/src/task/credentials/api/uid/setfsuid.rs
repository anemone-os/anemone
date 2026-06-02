//! setfsuid system call.

use crate::{prelude::*, task::credentials::Uid};

use super::{super::id::UserTarget, update_caps_by_fsuid};

/// Changes the current task's filesystem user ID and returns the previous one.
///
/// Permission check: the requested filesystem user ID must match one of the
/// current real/effective/saved/filesystem user IDs, unless the caller has
/// `CAP_SETUID`. Permission failure is a no-op and is reported only by
/// returning the old filesystem user ID.
///
/// This reuses `UserTarget` only for syscall argument decoding: `(uid_t)-1`
/// reaches the kernel as the invalid uid value used by the existing ID-target
/// parser. For this syscall it is not a setresuid-style per-field no-change
/// command; it is an invalid target, so the operation is a no-op.
///
/// Reference: <https://man7.org/linux/man-pages/man2/setfsuid.2.html>.
#[syscall(SYS_SETFSUID)]
fn sys_setfsuid(uid: UserTarget<Uid>) -> Result<u64, SysError> {
    let task = get_current_task();
    let old_fsuid = task.cred().uid.fs.get() as u64;
    let Some(uid) = uid.specified() else {
        knoticeln!(
            "setfsuid denied: invalid uid target, old fsuid={}",
            old_fsuid
        );
        return Ok(old_fsuid);
    };
    let _ = task.update_cred_with(|old| {
        let old_fsuid = old.uid.fs;
        if old.uid.matches_any_res(uid)
            || uid == old.uid.fs
            || old.has_cap_effective(Capability::SETUID)
        {
            if uid != old.uid.fs {
                old.uid.fs = uid;
                update_caps_by_fsuid(old, old_fsuid);
            }
            Ok(())
        } else {
            knoticeln!(
                "setfsuid denied: requested={}, uid={}, euid={}, suid={}, fsuid={}, missing={:?}",
                uid.get(),
                old.uid.real.get(),
                old.uid.effective.get(),
                old.uid.saved.get(),
                old.uid.fs.get(),
                Capability::SETUID
            );
            Ok(())
        }
    });
    Ok(old_fsuid)
}
