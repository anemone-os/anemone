//! pselect6 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/pselect6.2.html

use crate::{
    prelude::*,
    syscall::user_access::{SyscallArgValidatorExt as _, user_addr},
};

#[syscall(SYS_PSELECT6)]
pub fn sys_pselect6(
    n: i32,
    #[validate_with(user_addr.nullable())] inp: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] outp: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] exp: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] tsp: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] sig: Option<VirtAddr>,
) -> Result<u64, SysError> {
    todo!()
}
