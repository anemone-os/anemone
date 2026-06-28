use crate::prelude::*;

// stub.
#[syscall(SYS_UMASK)]
fn sys_umask(mask: u32) -> Result<u64, SysError> {
    kdebugln!("sys_umask: mask={:#o}", mask);

    Ok(0o777)
}
