//! brk system call.
//!
//! Reference:
//! - https://man7.org/linux/man-pages/man2/brk.2.html

use anemone_abi::syscall::SYS_BRK;
use kernel_macros::syscall;

use crate::{
    prelude::{dt::user_addr, *},
    sched::get_current_task,
};

#[syscall(SYS_BRK)]
fn sys_brk(#[validate_with(user_addr)] addr: VirtAddr) -> Result<u64, SysError> {
    let task = get_current_task();
    let memsp = task.clone_uspace();
    let brk = memsp
        .set_brk(addr)
        .unwrap_or_else(|_| memsp.read().brk())
        .get();

    return Ok(brk);
}
