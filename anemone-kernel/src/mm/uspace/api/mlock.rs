//! mlock system call.

use crate::prelude::{user_access::user_addr, *};

use super::checked_user_page_range;

#[syscall(SYS_MLOCK)]
fn sys_mlock(#[validate_with(user_addr)] addr: VirtAddr, len: u64) -> Result<u64, SysError> {
    let Some(range) = checked_user_page_range(addr, len)? else {
        return Ok(0);
    };

    let usp = get_current_task().clone_uspace_handle();
    usp.validate_mapped_range(range)?;
    Ok(0)
}
