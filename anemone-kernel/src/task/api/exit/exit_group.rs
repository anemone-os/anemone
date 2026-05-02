use crate::prelude::*;

/// Temporary workaround. now we don't have thread groups yet.
#[syscall(SYS_EXIT_GROUP)]
fn sys_exit_group(exit_code: i8) -> Result<u64, SysError> {
    knoticeln!("[NYI] exit_group: exit_code={}", exit_code);
    exit::kernel_exit(exit_code)
}
