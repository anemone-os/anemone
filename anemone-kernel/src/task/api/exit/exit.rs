use crate::{prelude::*, task::exit::kernel_exit};

// A successful exit never returns through the generated wrapper, so it has no
// completed invocation for the syscall profiler.
#[syscall(SYS_EXIT, profile = false)]
fn sys_exit(exit_code: i8) -> Result<u64, SysError> {
    kernel_exit(ExitCode::Exited(exit_code))
}
