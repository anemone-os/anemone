// TODO

use crate::prelude::*;

#[syscall(SYS_SET_ROBUST_LIST)]
fn sys_set_robust_list(_head: usize, _len: usize) -> Result<u64, SysError> {
    knoticeln!("[NYI] set_robust_list: head={:#x}, len={}", _head, _len);
    Err(SysError::NotYetImplemented)
}
