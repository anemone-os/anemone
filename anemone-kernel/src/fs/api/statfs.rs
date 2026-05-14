//! statfs system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/statfs.2.html

use crate::{
    prelude::*,
    syscall::user_access::{UserWritePtr, user_addr},
};

use anemone_abi::fs::linux::stat::StatFs as LinuxStatFs;

// stub. implement this later when we have time.
#[syscall(SYS_STATFS)]
fn sys_statfs(
    #[validate_with(user_addr)] pathname: VirtAddr,
    #[validate_with(user_addr)] buf: VirtAddr,
) -> Result<u64, SysError> {
    kdebugln!("sys_statfs: pathname={pathname:?}, buf={buf:?}",);

    let task = get_current_task();
    let usp_handle = task.clone_uspace_handle();

    {
        let mut usp = usp_handle.lock();
        UserWritePtr::<LinuxStatFs>::try_new(buf, &mut usp)?.write(LinuxStatFs::default());
    }

    Ok(0)
}
