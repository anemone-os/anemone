use crate::prelude::*;

use super::target::{PriorityWhich, collect_priority_targets};

/// Change selected tasks' nice value.
///
/// Permission check: the caller effective UID must match the target real or
/// effective UID, unless the caller has `CAP_SYS_NICE`. Requests that improve
/// priority additionally require `CAP_SYS_NICE` until `RLIMIT_NICE` is
/// implemented.
///
/// Reference: <https://man7.org/linux/man-pages/man2/setpriority.2.html>.
#[syscall(SYS_SETPRIORITY)]
fn sys_setpriority(which: PriorityWhich, who: i32, niceval: i32) -> Result<u64, SysError> {
    // Linux clamps setpriority arguments instead of rejecting out-of-range
    // values. This normalization remains at the ABI boundary.
    let nice = Nice::clamp(niceval);
    let current = get_current_task();
    let current_cred = current.cred();
    let targets = collect_priority_targets(&current, which, who)?;

    let mut result = Err(SysError::NoSuchProcess);
    for target in targets {
        result = set_one_priority(&current_cred, &target, nice, result);
    }

    result.map(|()| 0)
}

fn set_one_priority(
    current_cred: &CredentialSet,
    target: &Task,
    nice: Nice,
    previous: Result<(), SysError>,
) -> Result<(), SysError> {
    let target_cred = target.cred();

    if target_cred.uid.real != current_cred.uid.effective
        && target_cred.uid.effective != current_cred.uid.effective
        && !current_cred.has_cap_effective(Capability::SYS_NICE)
    {
        return Err(SysError::PermissionDenied);
    }

    if nice < target.nice() && !current_cred.has_cap_effective(Capability::SYS_NICE) {
        knoticeln!(
            "setpriority: denying task={} nice {} -> {} because RLIMIT_NICE is not implemented and caller lacks CAP_SYS_NICE",
            target.tid(),
            target.nice().get(),
            nice.get(),
        );
        return Err(SysError::AccessDenied);
    }

    target.set_nice(nice);
    if previous == Err(SysError::NoSuchProcess) {
        Ok(())
    } else {
        previous
    }
}
