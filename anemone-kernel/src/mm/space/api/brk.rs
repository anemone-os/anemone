use anemone_abi::syscall::SYS_BRK;
use kernel_macros::syscall;

use crate::{
    prelude::{
        dt::{nullable, user_addr},
        *,
    },
    sched::clone_current_task,
};

/// Handle the `brk` system call for the current task.
#[syscall(SYS_BRK)]
pub fn sys_brk(
    #[validate_with(nullable(user_addr))] addr: Option<VirtAddr>,
) -> Result<u64, SysError> {
    let task = clone_current_task();
    let memsp = task.clone_uspace().ok_or(MmError::NotMapped)?;

    if let Some(addr) = addr {
        memsp.set_brk(addr)?;
        Ok(0)
    } else {
        let brk = memsp.brk();
        debug_assert!(brk.get() < KernelLayout::USPACE_TOP_ADDR);
        Ok(brk.get())
    }
}
