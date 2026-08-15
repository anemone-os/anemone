//! sync system call.

use crate::prelude::*;

#[syscall(SYS_SYNC)]
fn sys_sync() -> Result<u64, SysError> {
    // There are no arguments to validate. Preserve the success-no-op ABI until
    // the filesystem owner exposes a global writeback operation.
    kdebugln!("sync is not implemented; returning success");
    Ok(0)
}
