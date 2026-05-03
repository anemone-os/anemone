use crate::{prelude::*, task::exit::kernel_exit_group};

#[syscall(SYS_EXIT_GROUP)]
fn sys_exit_group(exit_code: i8) -> Result<u64, SysError> {
    kernel_exit_group(ExitCode::Exited(exit_code))
}
