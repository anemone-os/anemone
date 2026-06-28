//! setgid system call.

use crate::{prelude::*, task::credentials::Gid};

/// Sets the current task's group IDs.
///
/// Permission check: with `CAP_SETGID`, real/effective/saved/filesystem group
/// IDs are all set to the requested ID. Without `CAP_SETGID`, the requested ID
/// must match the current real or saved group ID, and only the effective and
/// filesystem group IDs are changed.
///
/// Reference: <https://man7.org/linux/man-pages/man2/setgid.2.html>.
#[syscall(SYS_SETGID)]
fn sys_setgid(gid: Gid) -> Result<u64, SysError> {
    kdebugln!("setgid: gid={}", gid);

    let task = get_current_task();
    task.update_cred_with(|old| {
        if old.has_cap_effective(Capability::SETGID) {
            old.gid.real = gid;
            old.gid.effective = gid;
            old.gid.saved = gid;
            old.gid.fs = gid;
        } else if gid == old.gid.real || gid == old.gid.saved {
            old.gid.effective = gid;
            old.gid.fs = gid;
        } else {
            return Err(deny_permission!(
                "setgid denied: requested={}, gid={}, egid={}, sgid={}, missing={:?}",
                gid.get(),
                old.gid.real.get(),
                old.gid.effective.get(),
                old.gid.saved.get(),
                Capability::SETGID
            ));
        }
        Ok(())
    })?;
    Ok(0)
}
