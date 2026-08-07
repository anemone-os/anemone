//! `timer_create` system call.

use anemone_abi::time::linux::SigEvent;

use crate::{
    prelude::*,
    syscall::user_access::{UserReadPtr, UserWritePtr, user_addr},
    task::task_posix_timer::PosixTimerNotification,
};

use super::{notification_from_uapi, timer_clock};

#[syscall(SYS_TIMER_CREATE)]
fn sys_timer_create(
    clock_id: i32,
    event_ptr: u64,
    #[validate_with(user_addr)] timer_id_ptr: VirtAddr,
) -> Result<u64, SysError> {
    let clock = timer_clock(clock_id)?;
    let task = get_current_task();
    let owner = task.get_thread_group();
    let notification = if event_ptr == 0 {
        PosixTimerNotification::DefaultSignal
    } else {
        let event = {
            let event_ptr = user_addr(event_ptr)?;
            let usp_handle = task.clone_uspace_handle();
            let mut usp = usp_handle.lock();
            UserReadPtr::<SigEvent>::try_new(event_ptr, &mut usp)?.read()?
        };
        notification_from_uapi(&owner, event)?
    };

    let prepared = owner.prepare_posix_timer(clock, notification)?;
    let timer_id = prepared.id();
    {
        let usp_handle = task.clone_uspace_handle();
        let mut usp = usp_handle.lock();
        UserWritePtr::<i32>::try_new(timer_id_ptr, &mut usp)?.write(timer_id)?;
    }
    prepared.publish()?;
    Ok(0)
}
