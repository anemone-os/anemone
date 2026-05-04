use anemone_abi::syscall::SYS_GETPPID;
use kernel_macros::syscall;

use crate::prelude::*;

#[syscall(SYS_GETPPID)]
pub fn sys_getppid() -> Result<u64, SysError> {
    if let Some(ppid) = get_current_task().get_thread_group().parent_tgid() {
        Ok(ppid.get() as u64)
    } else {
        // return 0 for init thread group.
        Ok(0)
    }
}
