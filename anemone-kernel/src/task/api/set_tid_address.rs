//! set_tid_address system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/set_tid_address.2.html

use crate::{
    prelude::{dt::UserWritePtr, *},
    task::tid::Tid,
};

#[syscall(SYS_SET_TID_ADDRESS)]
fn sys_set_tid_address(tidptr: Option<UserWritePtr<Tid>>) -> Result<u64, SysError> {
    kdebugln!("set_tid_address: tidptr={:#x?}", tidptr.map(|p| p.addr()));
    get_current_task().set_clear_child_tid(tidptr);
    Ok(current_task_id().get() as u64)
}
