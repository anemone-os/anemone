use anemone_abi::process::linux::futex::RobustListHead;

use crate::{
    prelude::*,
    syscall::{
        handler::TryFromSyscallArg,
        user_access::{UserWritePtr, user_addr},
    },
};

#[derive(Debug)]
enum Target {
    CurrentTask,
    SpecifiedTask(Tid),
}

impl TryFromSyscallArg for Target {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        if raw == 0 {
            Ok(Self::CurrentTask)
        } else {
            Ok(Self::SpecifiedTask(Tid::try_from_syscall_arg(raw)?))
        }
    }
}

#[syscall(SYS_GET_ROBUST_LIST)]
fn sys_get_robust_list(
    target: Target,
    #[validate_with(user_addr)] head_ptr: VirtAddr,
    #[validate_with(user_addr)] len_ptr: VirtAddr,
) -> Result<u64, SysError> {
    kdebugln!(
        "sys_get_robust_list: target={:?}, head_ptr={:#x?}, len_ptr={:#x?}",
        target,
        head_ptr,
        len_ptr
    );

    let target = match target {
        Target::CurrentTask => get_current_task(),
        Target::SpecifiedTask(tid) => {
            let task = get_task(&tid).ok_or(SysError::NoSuchProcess)?;
            // A specified TID is a user-facing resolver and must not expose
            // kthread task-local state as an inert robust-list view.
            if task.get_thread_group().ty() == ThreadGroupType::KThread {
                return Err(SysError::NoSuchProcess);
            }
            task
        },
    };
    let head = target.robust_list();

    let current = get_current_task();
    let usp_handle = current.clone_uspace_handle();
    let mut usp = usp_handle.lock();

    UserWritePtr::<u64>::try_new(head_ptr, &mut usp)?
        .write(head.map(|head| head.get()).unwrap_or(0));
    UserWritePtr::<usize>::try_new(len_ptr, &mut usp)?.write(size_of::<RobustListHead>());

    Ok(0)
}
