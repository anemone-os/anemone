//! brk system call.
//!
//! Reference:
//! - https://man7.org/linux/man-pages/man2/brk.2.html

use anemone_abi::syscall::SYS_BRK;
use kernel_macros::syscall;

use crate::{
    prelude::{user_access::user_addr, *},
    sched::get_current_task,
};

#[syscall(SYS_BRK)]
fn sys_brk(#[validate_with(user_addr)] addr: VirtAddr) -> Result<u64, SysError> {
    let task = get_current_task();
    let usp = task.clone_uspace_handle();

    let brk = usp.set_brk(addr);

    let brk = match brk {
        Ok(_guard) => addr.get(),
        Err(e) => usp.lock().brk().get(),
    };

    return Ok(brk);
}
