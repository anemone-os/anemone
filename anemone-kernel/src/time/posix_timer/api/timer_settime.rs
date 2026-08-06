//! `timer_settime` system call.

use anemone_abi::time::linux::{ITimerSpec, clock::TIMER_ABSTIME};

use crate::{
    prelude::*,
    syscall::user_access::{SyscallArgValidatorExt as _, UserReadPtr, UserWritePtr, user_addr},
};

use super::{setting_from_uapi, setting_to_uapi};

#[syscall(SYS_TIMER_SETTIME)]
fn sys_timer_settime(
    timer_id: i32,
    flags: i32,
    #[validate_with(user_addr)] new_ptr: VirtAddr,
    old_ptr: u64,
) -> Result<u64, SysError> {
    // Linux accepts every non-TIMER_ABSTIME bit, so record this compatibility
    // choice once without turning normal legacy use into a persistent warning.
    static IGNORED_SETTIME_FLAGS_LOGGED: AtomicBool = AtomicBool::new(false);

    let task = get_current_task();
    let new_setting = {
        let usp_handle = task.clone_uspace_handle();
        let mut usp = usp_handle.lock();
        let value = UserReadPtr::<ITimerSpec>::try_new(new_ptr, &mut usp)?.read()?;
        setting_from_uapi(value)?
    };

    let ignored_flags = flags & !TIMER_ABSTIME;
    if ignored_flags != 0 && !IGNORED_SETTIME_FLAGS_LOGGED.swap(true, Ordering::Relaxed) {
        // Linux's legacy timer_settime ABI tests only TIMER_ABSTIME and ignores
        // every other bit. Preserve that visible behavior, but keep it
        // observable so a future ABI revision cannot accidentally tighten it.
        knoticeln!(
            "timer_settime: ignoring legacy flag bits {:#x}",
            ignored_flags
        );
    }
    let old_setting = task.get_thread_group().posix_timer_settime(
        timer_id,
        new_setting,
        flags & TIMER_ABSTIME != 0,
    )?;

    // Linux applies the new setting before old-value copyout. An EFAULT here
    // is fail-forward and must not roll the timer back to its previous arm.
    if let Some(old_ptr) = (user_addr.nullable())(old_ptr)? {
        let usp_handle = task.clone_uspace_handle();
        let mut usp = usp_handle.lock();
        UserWritePtr::<ITimerSpec>::try_new(old_ptr, &mut usp)?
            .write(setting_to_uapi(old_setting))?;
    }
    Ok(0)
}
