use crate::{prelude::*, task::exit::kernel_exit_group};

// A successful group exit never returns through the generated wrapper, so it
// has no completed invocation for the syscall profiler.
#[syscall(SYS_EXIT_GROUP, profile = false)]
fn sys_exit_group(exit_code: i8) -> Result<u64, SysError> {
    kernel_exit_group(ExitCode::Exited(exit_code))
}
