//! mlock system call.

use crate::prelude::{user_access::user_addr, *};

use super::checked_user_page_range;

#[syscall(SYS_MLOCK)]
fn sys_mlock(#[validate_with(user_addr)] addr: VirtAddr, len: u64) -> Result<u64, SysError> {
    let Some(range) = checked_user_page_range(addr, len)? else {
        return Ok(0);
    };

    let usp = get_current_task().clone_uspace_handle();
    // Placeholder semantics: we only verify that the pages exist. There is no
    // swap-backed lock accounting yet, so no VM_LOCKED state is recorded here.
    usp.validate_mapped_range(range)?;
    Ok(0)
}
