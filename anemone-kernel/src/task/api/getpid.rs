use anemone_abi::syscall::SYS_GETPID;
use kernel_macros::syscall;

use crate::prelude::*;

#[syscall(SYS_GETPID)]
pub fn sys_getpid() -> Result<u64, SysError> {
    Ok(get_current_task().tgid().get() as u64)
}
