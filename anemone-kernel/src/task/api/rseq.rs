use crate::prelude::*;

#[syscall(SYS_RSEQ)]
fn sys_rseq(
    _rseq_addr: u64,
    _rseq_len: u64,
    _flags: u64,
    _signature: u64,
) -> Result<u64, SysError> {
    // Raw-width parameters ensure conversion cannot return another errno before
    // this registered ABI stub ignores every argument and returns ENOSYS. The
    // stub has no side effects; replace it only when task-owned rseq state and
    // its full clone, exec, exit, and scheduler lifecycle are implemented.
    Err(SysError::NotYetImplemented)
}
