//! prlimit64 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/prlimit64.2.html

use anemone_abi::process::linux::resource::RLimit;

use crate::{
    prelude::*,
    syscall::{
        handler::TryFromSyscallArg,
        user_access::{SyscallArgValidatorExt as _, UserReadPtr, UserWritePtr, user_addr},
    },
    task::{credentials::cap::Capability, task_resource::RLimitResource},
};

#[derive(Debug)]
enum PrLimitTarget {
    SelfProcess,
    Task(Tid),
}

impl TryFromSyscallArg for PrLimitTarget {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        match raw {
            0 => Ok(Self::SelfProcess),
            tid => Ok(Self::Task(Tid::try_from_syscall_arg(tid)?)),
        }
    }
}

struct ResolvedPrLimitTarget {
    task: Arc<Task>,
    thread_group: Arc<ThreadGroup>,
}

fn resolve_target(target: PrLimitTarget) -> Result<ResolvedPrLimitTarget, SysError> {
    let (task, thread_group) = match target {
        PrLimitTarget::SelfProcess => {
            let task = get_current_task();
            let thread_group = task.get_thread_group();
            (task, thread_group)
        },
        // Linux accepts any live TID here, then applies the operation to the
        // process-wide resource policy shared by that task's thread group.
        PrLimitTarget::Task(tid) => {
            get_task_and_thread_group(&tid).ok_or(SysError::NoSuchProcess)?
        },
    };
    if thread_group.ty() != ThreadGroupType::User {
        return Err(SysError::NoSuchProcess);
    }
    Ok(ResolvedPrLimitTarget { task, thread_group })
}

fn check_target_permission(target: &Task) -> Result<(), SysError> {
    let current = get_current_task();
    if current.tid() == target.tid() || current.has_cap(Capability::SYS_RESOURCE) {
        return Ok(());
    }

    let caller = current.cred();
    let target = target.cred();
    // Linux permits an unprivileged cross-process operation only when the
    // caller's real IDs match every real/effective/saved ID of the target.
    let uid_match = target.uid.real == caller.uid.real
        && target.uid.effective == caller.uid.real
        && target.uid.saved == caller.uid.real;
    let gid_match = target.gid.real == caller.gid.real
        && target.gid.effective == caller.gid.real
        && target.gid.saved == caller.gid.real;
    if uid_match && gid_match {
        Ok(())
    } else {
        Err(SysError::PermissionDenied)
    }
}

#[syscall(SYS_PRLIMIT64)]
fn sys_prlimit64(
    target: PrLimitTarget,
    resource: RLimitResource,
    #[validate_with(user_addr.nullable())] new_limit: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] old_limit: Option<VirtAddr>,
) -> Result<u64, SysError> {
    kdebugln!(
        "prlimit64: target={:?}, resource={:?}, new_limit={:?}, old_limit={:?}",
        target,
        resource,
        new_limit,
        old_limit
    );

    let task = get_current_task();
    let target = resolve_target(target)?;
    check_target_permission(&target.task)?;

    let usp_handle = task.clone_uspace_handle();
    let proposed = if let Some(new_limit) = new_limit {
        let mut usp = usp_handle.lock();
        Some(
            UserReadPtr::<RLimit>::try_new(new_limit, &mut usp)?
                .read()?
                .into(),
        )
    } else {
        None
    };

    let old = if let Some(proposed) = proposed {
        target.thread_group.update_rlimit(
            resource,
            proposed,
            task.has_cap(Capability::SYS_RESOURCE),
        )?
    } else {
        target.thread_group.read_rlimit(resource)?
    };

    if let Some(old_limit) = old_limit {
        let mut usp = usp_handle.lock();
        // Linux exposes the pre-update pair. A later copyout failure does not
        // roll back the already committed policy transaction.
        UserWritePtr::<RLimit>::try_new(old_limit, &mut usp)?.write(old.into_abi())?;
    }

    Ok(0)
}
