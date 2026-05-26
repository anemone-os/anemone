//! sync system call.

use crate::prelude::*;

#[syscall(SYS_SYNC)]
fn sys_sync() -> Result<u64, SysError> {
    Ok(0)
}
