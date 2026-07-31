use anemone_abi::{syscall::SYS_TIMERFD_GETTIME, time::linux::ITimerSpec};

use crate::{
    fs::timerfd::gettime,
    prelude::{
        user_access::{UserWritePtr, user_addr},
        *,
    },
    task::files::Fd,
};

#[syscall(SYS_TIMERFD_GETTIME)]
fn sys_timerfd_gettime(
    fd: Fd,
    #[validate_with(user_addr)] curr_value: VirtAddr,
) -> Result<u64, SysError> {
    let task = get_current_task();
    let file = task.get_fd(fd)?;
    let snapshot = gettime(file.vfs_file())?;

    let uspace = task.clone_uspace_handle();
    let mut usp = uspace.lock();
    UserWritePtr::<ITimerSpec>::try_new(curr_value, &mut usp)?.write(snapshot);

    Ok(0)
}
