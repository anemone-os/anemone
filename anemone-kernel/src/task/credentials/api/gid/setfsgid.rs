//! setfsgid system call.

use crate::{prelude::*, task::credentials::Gid};

use super::super::id::UserTarget;

/// Changes the current task's filesystem group ID and returns the previous one.
///
/// Permission check: the requested filesystem group ID must match one of the
/// current real/effective/saved/filesystem group IDs, unless the caller has
/// `CAP_SETGID`. Permission failure is a no-op and is reported only by
/// returning the old filesystem group ID.
///
/// This reuses `UserTarget` only for syscall argument decoding: `(gid_t)-1`
/// reaches the kernel as the invalid gid value used by the existing ID-target
/// parser. For this syscall it is not a setresgid-style per-field no-change
/// command; it is an invalid target, so the operation is a no-op.
///
/// Reference: <https://man7.org/linux/man-pages/man2/setfsgid.2.html>.
#[syscall(SYS_SETFSGID)]
fn sys_setfsgid(gid: UserTarget<Gid>) -> Result<u64, SysError> {
    let task = get_current_task();
    let old_fsgid = task.cred().gid.fs.get() as u64;
    let Some(gid) = gid.specified() else {
        knoticeln!(
            "setfsgid denied: invalid gid target, old fsgid={}",
            old_fsgid
        );
        return Ok(old_fsgid);
    };
    let _ = task.update_cred_with(|old| {
        if old.gid.matches_any_res(gid)
            || gid == old.gid.fs
            || old.has_cap_effective(Capability::SETGID)
        {
            if gid != old.gid.fs {
                old.gid.fs = gid;
            }
            Ok(())
        } else {
            knoticeln!(
                "setfsgid denied: requested={}, gid={}, egid={}, sgid={}, fsgid={}, missing={:?}",
                gid.get(),
                old.gid.real.get(),
                old.gid.effective.get(),
                old.gid.saved.get(),
                old.gid.fs.get(),
                Capability::SETGID
            );
            Ok(())
        }
    });
    Ok(old_fsgid)
}
