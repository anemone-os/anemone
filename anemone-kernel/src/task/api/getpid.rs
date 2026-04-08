use anemone_abi::syscall::SYS_GETPID;
use kernel_macros::syscall;

use crate::{prelude::*, sched::current_task_id};

#[syscall(SYS_GETPID)]
pub fn sys_getpid() -> Result<u64, SysError> {
    let res = Ok(current_task_id().get() as u64);
    res
}
