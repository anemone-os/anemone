//! Process nice-value priority syscalls.

use crate::prelude::{handler::TryFromSyscallArg, *};

const MIN_NICE: i32 = -20;
const MAX_NICE: i32 = 19;

#[derive(Debug, Clone, Copy)]
enum PriorityWhich {
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

/// Change selected tasks' nice value.
///
/// Permission check: the caller effective UID must match the target real or
/// effective UID, unless the caller has `CAP_SYS_NICE`. Requests that improve
/// priority below the target's current nice value additionally require
/// `CAP_SYS_NICE` until `RLIMIT_NICE` is implemented.
///
/// Reference: <https://man7.org/linux/man-pages/man2/setpriority.2.html>.
#[syscall(SYS_SETPRIORITY)]
fn sys_setpriority(which: PriorityWhich, who: i32, niceval: i32) -> Result<u64, SysError> {
    let niceval = niceval.clamp(MIN_NICE, MAX_NICE);
    let current = get_current_task();
    let targets = collect_priority_targets(&current, which, who)?;

    let mut ret = Err(SysError::NoSuchProcess);
    for target in targets {
        ret = set_one_priority(&current, &target, niceval, ret);
    }

    ret.map(|()| 0)
}

/// Return the highest priority among selected tasks.
///
/// The syscall ABI returns `20 - nice` so successful negative nice values do
/// not collide with negative errno returns.
///
/// Reference: <https://man7.org/linux/man-pages/man2/getpriority.2.html>.
#[syscall(SYS_GETPRIORITY)]
fn sys_getpriority(which: PriorityWhich, who: i32) -> Result<u64, SysError> {
    let current = get_current_task();
    let targets = collect_priority_targets(&current, which, who)?;
    let mut highest = 0;
    for target in targets {
        highest = highest.max(nice_to_syscall_return(target.nice()));
    }
    Ok(highest as u64)
}

fn collect_priority_targets(
    current: &Arc<Task>,
    which: PriorityWhich,
    who: i32,
) -> Result<Vec<Arc<Task>>, SysError> {
    match which {
        PriorityWhich::Process => {
            let task = if who == 0 {
                current.clone()
            } else if who > 0 {
                get_task(&Tid::new(who as u32)).ok_or(SysError::NoSuchProcess)?
            } else {
                return Err(SysError::NoSuchProcess);
            };
            Ok(vec![task])
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
                if let Some(leader) = tg.leader() {
                    targets.push(leader);
                }
            }
            if targets.is_empty() {
                Err(SysError::NoSuchProcess)
            } else {
                Ok(targets)
            }
        },
        PriorityWhich::User => {
            let uid = if who == 0 {
                current.cred().uid.real
            } else if who > 0 {
                Uid::new(who as u32)
            } else {
                return Err(SysError::NoSuchProcess);
            };
            let mut targets = Vec::new();
            for_each_task(|task| {
                if task.cred().uid.real == uid && task.tid() != Tid::IDLE {
                    targets.push(task.clone());
                }
            });
            if targets.is_empty() {
                Err(SysError::NoSuchProcess)
            } else {
                Ok(targets)
            }
        },
    }
}

fn set_one_priority(
    current: &Task,
    target: &Task,
    niceval: i32,
    previous: Result<(), SysError>,
) -> Result<(), SysError> {
    let current_cred = current.cred();
    let target_cred = target.cred();

    if target_cred.uid.real != current_cred.uid.effective
        && target_cred.uid.effective != current_cred.uid.effective
        && !current_cred.has_cap_effective(Capability::SYS_NICE)
    {
        return Err(SysError::PermissionDenied);
    }

    if niceval < target.nice() as i32 && !current_cred.has_cap_effective(Capability::SYS_NICE) {
        return Err(SysError::AccessDenied);
    }

    kernel_setpriority(target, niceval)?;
    if previous == Err(SysError::NoSuchProcess) {
        Ok(())
    } else {
        previous
    }
}

pub fn kernel_setpriority(target: &Task, niceval: i32) -> Result<(), SysError> {
    target.set_nice(niceval as isize);
    Ok(())
}

fn nice_to_syscall_return(nice: isize) -> isize {
    20 - nice
}
