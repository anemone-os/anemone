use anemone_abi::{syscall::SYS_TIMERFD_SETTIME, time::linux::ITimerSpec};

use crate::{
    fs::timerfd::{settime, validate_settime_value},
    prelude::{
        user_access::{SyscallArgValidatorExt as _, UserReadPtr, UserWritePtr, user_addr},
        *,
    },
    task::files::Fd,
};

use super::TimerFdSettimeSysFlags;

#[syscall(SYS_TIMERFD_SETTIME)]
fn sys_timerfd_settime(
    fd: Fd,
    flags: TimerFdSettimeSysFlags,
    #[validate_with(user_addr)] new_value: VirtAddr,
    #[validate_with(user_addr.nullable())] old_value: Option<VirtAddr>,
) -> Result<u64, SysError> {
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let new_value = {
        let mut usp = uspace.lock();
        UserReadPtr::<ITimerSpec>::try_new(new_value, &mut usp)?.read()
    };
    validate_settime_value(new_value)?;

    let file = task.get_fd(fd)?;
    let old_snapshot = settime(file.vfs_file(), flags.into(), new_value)?;

    if let Some(old_value) = old_value {
        let mut usp = uspace.lock();
        UserWritePtr::<ITimerSpec>::try_new(old_value, &mut usp)?.write(old_snapshot);
    }

    Ok(0)
}
