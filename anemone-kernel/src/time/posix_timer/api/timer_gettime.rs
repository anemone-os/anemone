//! `timer_gettime` system call.

use anemone_abi::time::linux::ITimerSpec;

use crate::{
    prelude::*,
    syscall::user_access::{UserWritePtr, user_addr},
};

use super::setting_to_uapi;

#[syscall(SYS_TIMER_GETTIME)]
fn sys_timer_gettime(
    timer_id: i32,
    #[validate_with(user_addr)] current_ptr: VirtAddr,
) -> Result<u64, SysError> {
    let task = get_current_task();
    let setting = task.get_thread_group().posix_timer_gettime(timer_id)?;
    let current = setting_to_uapi(setting);
    let usp_handle = task.clone_uspace_handle();
    let mut usp = usp_handle.lock();
    UserWritePtr::<ITimerSpec>::try_new(current_ptr, &mut usp)?.write(current)?;
    Ok(0)
}
