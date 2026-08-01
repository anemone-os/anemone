//! getrlimit system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/getrlimit.2.html

use anemone_abi::process::linux::resource::RLimit;

use crate::{
    prelude::*,
    syscall::user_access::{UserWritePtr, user_addr},
    task::task_resource::RLimitResource,
};

#[syscall(SYS_GETRLIMIT)]
fn sys_getrlimit(
    resource: RLimitResource,
    #[validate_with(user_addr)] rlim: VirtAddr,
) -> Result<u64, SysError> {
    kdebugln!("getrlimit: resource={:?}, rlim={:?}", resource, rlim);

    let task = get_current_task();
    let rlimit: RLimit = task.get_thread_group().read_rlimit(resource)?.into_abi();

    let usp_handle = task.clone_uspace_handle();
    let mut usp = usp_handle.lock();

    UserWritePtr::<RLimit>::try_new(rlim, &mut usp)?.write(rlimit)?;

    Ok(0)
}
