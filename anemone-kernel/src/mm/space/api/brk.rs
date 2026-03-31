use anemone_abi::syscall::SYS_BRK;
use kernel_macros::syscall;

use crate::{
    prelude::{dt::user_vaddr, *},
    sched::clone_current_task,
};

#[syscall(SYS_BRK)]
pub fn sys_brk(#[validate_with(user_vaddr)] addr: VirtAddr) -> Result<u64, SysError> {
    let task = clone_current_task();
    let memsp = task.clone_uspace().ok_or(MmError::NotMapped)?;
    if addr == VirtAddr::new(0) {
        let brk = memsp.brk();
        debug_assert!(brk.get() < KernelLayout::USPACE_TOP_ADDR);
        Ok(brk.get())
    } else {
        memsp.set_brk(addr)?;
        Ok(0)
    }
}
