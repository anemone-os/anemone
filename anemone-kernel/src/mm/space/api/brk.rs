use anemone_abi::syscall::SYS_BRK;
use kernel_macros::syscall;

use crate::{
    prelude::{dt::user_nullable_vaddr, *},
    sched::clone_current_task,
};

/// Handle the `brk` system call for the current task.
#[syscall(SYS_BRK)]
pub fn sys_brk(#[validate_with(user_nullable_vaddr)] addr: VirtAddr) -> Result<u64, SysError> {
    let task = clone_current_task();
    let memsp = task.clone_uspace().ok_or(MmError::NotMapped)?;
    if addr == VirtAddr::new(0) {
        let brk = memsp.read().brk();
        debug_assert!(brk.get() < KernelLayout::USPACE_TOP_ADDR);
        Ok(brk.get())
    } else {
        memsp.write().set_brk(addr)?;
        Ok(0)
    }
}
