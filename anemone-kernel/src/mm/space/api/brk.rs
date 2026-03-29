use anemone_abi::syscall::SYS_BRK;
use kernel_macros::syscall;

use crate::{
    prelude::{MmError, SysError, VirtAddr, dt::user_vaddr},
    sched::with_current_task,
};

#[syscall(SYS_BRK)]
pub fn sys_brk(#[validate_with(user_vaddr)] addr: VirtAddr) -> Result<u64, SysError> {
    with_current_task(|task| -> Result<(), SysError> {
        let memsp = task.uspace().ok_or(MmError::NotMapped)?;
        memsp.set_brk(addr)?;
        Ok(())
    })?;
    Ok(0)
}
