//! Linux legacy scheduler policy, parameter, and query syscalls.

use core::mem::size_of;

use anemone_abi::process::linux::sched::SchedParam;

use crate::{
    prelude::{
        user_access::{UserReadSlice, UserWriteSlice, user_addr},
        *,
    },
    sched::{
        config::{SchedChangePermit, SchedError},
        request::{SubmitError, submit_config_patch},
    },
};

mod sched_get_priority_max;
mod sched_get_priority_min;
mod sched_getparam;
mod sched_getscheduler;
mod sched_rr_get_interval;
mod sched_setparam;
mod sched_setscheduler;

const _: () = assert!(size_of::<SchedParam>() == size_of::<i32>());

fn read_sched_param(addr: u64) -> Result<SchedParam, SysError> {
    let mut raw = [0u8; size_of::<i32>()];
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut usp = uspace.lock();
    let user = UserReadSlice::<u8>::try_new(user_addr(addr)?, raw.len(), &mut usp)?;
    user.copy_to_slice(&mut raw);
    Ok(SchedParam {
        sched_priority: i32::from_ne_bytes(raw),
    })
}

fn write_sched_param(addr: u64, param: SchedParam) -> Result<(), SysError> {
    let raw = param.sched_priority.to_ne_bytes();
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut usp = uspace.lock();
    let mut user = UserWriteSlice::<u8>::try_new(user_addr(addr)?, raw.len(), &mut usp)?;
    user.copy_from_slice(&raw);
    Ok(())
}

fn resolve_policy_target(pid: i32) -> Result<Arc<Task>, SysError> {
    Ok(match pid {
        0 => get_current_task(),
        pid if pid > 0 => get_task(&Tid::new(pid as u32)).ok_or(SysError::NoSuchProcess)?,
        _ => return Err(SysError::InvalidArgument),
    })
}

fn policy_change_permit(
    caller: &CredentialSet,
    target: &Task,
) -> Result<SchedChangePermit, SysError> {
    if target.flags().is_kernel() {
        return Err(SysError::NoSuchProcess);
    }
    let target = target.cred();
    let privileged = caller.has_cap_effective(Capability::SYS_NICE);
    if target.uid.real != caller.uid.effective
        && target.uid.effective != caller.uid.effective
        && !privileged
    {
        return Err(SysError::PermissionDenied);
    }

    Ok(if privileged {
        SchedChangePermit::unrestricted()
    } else {
        SchedChangePermit::non_escalating()
    })
}

fn submit_policy_patch(
    target: Arc<Task>,
    patch: crate::sched::config::SchedConfigPatch,
    permit: SchedChangePermit,
) -> Result<(), SysError> {
    submit_config_patch(target, patch, permit).map_err(map_submit_error)
}

fn map_submit_error(error: SubmitError) -> SysError {
    match error {
        SubmitError::Transaction(SchedError::TransitionDenied) => SysError::PermissionDenied,
        SubmitError::Transaction(SchedError::TargetExited) => SysError::NoSuchProcess,
        SubmitError::Transaction(SchedError::InvalidParameters | SchedError::InvalidAffinity) => {
            SysError::InvalidArgument
        },
        SubmitError::Transport(IpiError::Alloc(_)) => SysError::OutOfMemory,
        SubmitError::Transport(IpiError::TargetOffline) => SysError::NoSuchProcess,
        SubmitError::CompletionClosed => SysError::IO,
    }
}
