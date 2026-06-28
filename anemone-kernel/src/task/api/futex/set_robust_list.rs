use anemone_abi::process::linux::futex::RobustListHead;

use crate::{
    prelude::*,
    syscall::user_access::{SyscallArgValidatorExt as _, user_addr},
};

#[syscall(SYS_SET_ROBUST_LIST)]
fn sys_set_robust_list(
    #[validate_with(user_addr.nullable())] head: Option<VirtAddr>,
    len: usize,
) -> Result<u64, SysError> {
    kdebugln!("sys_set_robust_list: head={:?}, len={}", head, len);

    if len != size_of::<RobustListHead>() {
        return Err(SysError::InvalidArgument);
    }

    get_current_task().set_robust_list(head);

    Ok(0)
}
