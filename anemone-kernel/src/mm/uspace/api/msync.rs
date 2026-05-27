//! msync system call.

use crate::prelude::{
    user_access::{aligned_to, user_addr, SyscallArgValidatorExt},
    *,
};

use super::{args::MsyncFlags, checked_user_page_range};

#[syscall(SYS_MSYNC)]
fn sys_msync(
    #[validate_with(aligned_to::<{ PagingArch::PAGE_SIZE_BYTES }>.and_then(user_addr))]
    addr: VirtAddr,
    len: u64,
    flags: MsyncFlags,
) -> Result<u64, SysError> {
    if flags.contains(MsyncFlags::MS_INVALIDATE) {
        kwarningln!("msync: ignoring MS_INVALIDATE");
    }

    let Some(range) = checked_user_page_range(addr, len)? else {
        return Ok(0);
    };

    let usp = get_current_task().clone_uspace_handle();
    usp.sync_range(range)?;
    Ok(0)
}
