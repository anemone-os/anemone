use crate::prelude::{handler::TryFromSyscallArg, *};

#[derive(Debug, Clone, Copy)]
pub(super) enum PriorityWhich {
    Process,
    ProcessGroup,
    User,
}

impl TryFromSyscallArg for PriorityWhich {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        match i32::try_from_syscall_arg(raw)? {
            0 => Ok(Self::Process),
            1 => Ok(Self::ProcessGroup),
            2 => Ok(Self::User),
            _ => Err(SysError::InvalidArgument),
        }
    }
}

/// Snapshot the tasks selected by one Linux `getpriority` / `setpriority`
/// selector.
///
/// The snapshot deliberately releases topology and group locks before callers
/// inspect credentials or modify nice. It provides object consistency, not a
/// topology-wide atomic transaction: membership may change after selection.
/// Full selection/mutation linearizability would require a separate topology
/// protocol in addition to future owner-CPU scheduler commands and is not
/// provided by this first version.
pub(super) fn collect_priority_targets(
    current: &Arc<Task>,
    which: PriorityWhich,
    who: i32,
) -> Result<Vec<Arc<Task>>, SysError> {
    let targets = match which {
        PriorityWhich::Process => {
            let task = if who == 0 {
                current.clone()
            } else if who > 0 {
                get_task(&Tid::new(who as u32)).ok_or(SysError::NoSuchProcess)?
            } else {
                return Err(SysError::NoSuchProcess);
            };
            if task.flags().is_kernel() {
                return Err(SysError::NoSuchProcess);
            }
            vec![task]
        },
        PriorityWhich::ProcessGroup => {
            let pgid = if who == 0 {
                current.get_thread_group().pgid()
            } else if who > 0 {
                Tid::new(who as u32)
            } else {
                return Err(SysError::NoSuchProcess);
            };
            let process_group = get_process_group(&pgid).ok_or(SysError::NoSuchProcess)?;
            let mut targets = Vec::new();
            for tg in process_group.get_members() {
                if tg.ty() == ThreadGroupType::User {
                    targets.extend(tg.get_members());
                }
            }
            targets
        },
        PriorityWhich::User => {
            let uid = if who == 0 {
                current.cred().uid.real
            } else if who > 0 {
                Uid::new(who as u32)
            } else {
                return Err(SysError::NoSuchProcess);
            };

            // Do not inspect credentials while `for_each_task` holds the
            // topology read lock. Snapshot task identities first, then filter.
            let mut all_tasks = Vec::new();
            for_each_task(|task| all_tasks.push(task.clone()));
            all_tasks
                .into_iter()
                .filter(|task| {
                    task.cred().uid.real == uid
                        && task.tid() != Tid::IDLE
                        && !task.flags().is_kernel()
                })
                .collect()
        },
    };

    if targets.is_empty() {
        Err(SysError::NoSuchProcess)
    } else {
        kdebugln!(
            "priority target snapshot: which={:?} who={} tasks={}",
            which,
            who,
            targets.len(),
        );
        Ok(targets)
    }
}
