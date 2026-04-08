use anemone_abi::syscall::SYS_WAIT4;
use kernel_macros::syscall;

use crate::{
    prelude::{SysError, handler::TryFromSyscallArg},
    sched::clone_current_task,
    task::{ArcTaskImpls, WaitObject, tid::Tid},
};

impl TryFromSyscallArg for WaitObject {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = raw as i64;
        if raw < -1 {
            unimplemented!();
        } else if raw == -1 {
            Ok(WaitObject::Tid(None))
        } else if raw == 0 {
            unimplemented!();
        } else {
            Ok(WaitObject::Tid(Some(Tid::new(raw as u32))))
        }
    }
}

#[syscall(SYS_WAIT4)]
pub fn sys_wait4(target: WaitObject) -> Result<u64, SysError> {
    unsafe {
        let tid = clone_current_task().waitpid(target)?;
        Ok(tid.get() as u64)
    }
}
