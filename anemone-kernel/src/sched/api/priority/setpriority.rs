use crate::{
    prelude::*,
    sched::{
        config::{SchedChangePermit, SchedConfigPatch, SchedError},
        request::{SubmitError, submit_config_patch},
    },
};

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
        result = set_one_priority(&current_cred, target, nice, result);
    }

    result.map(|()| 0)
}

fn set_one_priority(
    current_cred: &CredentialSet,
    target: Arc<Task>,
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

    let permit = if current_cred.has_cap_effective(Capability::SYS_NICE) {
        SchedChangePermit::unrestricted()
    } else {
        SchedChangePermit::non_escalating()
    };
    let patch = SchedConfigPatch::keep().with_nice(nice);
    submit_config_patch(target, patch, permit).map_err(map_submit_error)?;
    if previous == Err(SysError::NoSuchProcess) {
        Ok(())
    } else {
        previous
    }
}

fn map_submit_error(error: SubmitError) -> SysError {
    match error {
        SubmitError::Transaction(SchedError::TransitionDenied) => SysError::AccessDenied,
        SubmitError::Transaction(SchedError::TargetExited) => SysError::NoSuchProcess,
        SubmitError::Transaction(SchedError::InvalidParameters | SchedError::InvalidAffinity) => {
            SysError::InvalidArgument
        },
        SubmitError::Transport(IpiError::Alloc(_)) => SysError::OutOfMemory,
        SubmitError::Transport(IpiError::TargetOffline) => SysError::NoSuchProcess,
        SubmitError::CompletionClosed => SysError::IO,
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::sched::class::SchedEntity;

    fn target() -> Arc<Task> {
        fn unused_entry() {}

        let (task, guard) = unsafe {
            Task::new_kernel(
                "kunit-setpriority",
                unused_entry as *const (),
                ParameterList::empty(),
                None,
                None,
                SchedEntity::new_default(),
                TaskFlags::empty(),
                Some(cur_cpu_id()),
            )
        }
        .expect("failed to construct setpriority KUnit task");
        unsafe {
            guard.forget();
        }
        Arc::new(task)
    }

    #[kunit]
    fn test_setpriority_local_path_and_partial_progress_folding() {
        let root = CredentialSet::new_root();
        let first = target();
        assert_eq!(
            set_one_priority(
                &root,
                first.clone(),
                Nice::new(5),
                Err(SysError::NoSuchProcess),
            ),
            Ok(())
        );
        assert_eq!(first.nice(), Nice::new(5));

        let later = target();
        assert_eq!(
            set_one_priority(&root, later.clone(), Nice::new(10), Err(SysError::IO),),
            Err(SysError::IO)
        );
        assert_eq!(later.nice(), Nice::new(10));
    }
}
