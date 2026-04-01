use anemone_abi::syscall::SYS_GETPPID;
use kernel_macros::syscall;

use crate::{prelude::*, sched::with_current_task, task::tid::Tid};

pub fn kernel_getppid() -> Option<Tid> {
    let parent = with_current_task(|task| task.parent_tid());
    parent
}

/// Return 0 if the current task has no parent. e.g. the init task, the idle
/// task, a exited task or the kinit task.
#[syscall(SYS_GETPPID)]
pub fn sys_getppid() -> Result<u64, SysError> {
    Ok(kernel_getppid().map_or(0, |tid| tid.get() as u64))
}
