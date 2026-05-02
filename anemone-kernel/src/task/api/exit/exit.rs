use crate::{prelude::*, task::exit::kernel_exit};

#[syscall(SYS_EXIT)]
fn sys_exit(exit_code: i8) -> Result<u64, SysError> {
    kernel_exit(exit_code)
}
