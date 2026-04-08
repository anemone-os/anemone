use anemone_abi::syscall::SYS_WAIT4;
use kernel_macros::syscall;

use crate::{
    prelude::{SysError, dt::UserWritePtr, handler::TryFromSyscallArg},
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

#[repr(C)]
pub struct WStatus {
    value: u16,
}
impl WStatus {
    pub fn normal(exit_code: i8) -> Self {
        WStatus {
            value: (exit_code as u16) << 8,
        }
    }
    // todo:
}

#[syscall(SYS_WAIT4)]
pub fn sys_wait4(
    target: WaitObject,
    wstatus: Option<UserWritePtr<WStatus>>,
) -> Result<u64, SysError> {
    let task = unsafe { clone_current_task().waitpid(target)? };
    if let Some(wstatus) = wstatus {
        wstatus.safe_write(WStatus::normal(task.exit_code()))?;
    }
    Ok(task.tid().get() as u64)
}
