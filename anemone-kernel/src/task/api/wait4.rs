use anemone_abi::{process::linux::wait, syscall::SYS_WAIT4};
use bitflags::bitflags;
use kernel_macros::syscall;

use crate::{
    prelude::{dt::UserWritePtr, handler::TryFromSyscallArg, *},
    sched::clone_current_task,
    task::{ArcTaskImpls, WaitObject, tid::Tid},
};

impl TryFromSyscallArg for WaitObject {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = raw as i64;
        if raw < -1 {
            // unimplemented: wait for any child in the same process group
            Err(SysError::NotYetImplemented)
        } else if raw == -1 {
            Ok(WaitObject::Tid(None))
        } else if raw == 0 {
            // unimplemented: wait for any child in the same process group
            Err(SysError::NotYetImplemented)
        } else {
            Ok(WaitObject::Tid(Some(Tid::new(raw as u32))))
        }
    }
}

#[repr(C)]
pub struct WStatus {
    value: u32,
}
impl WStatus {
    pub fn normal(exit_code: i8) -> Self {
        WStatus {
            value: (exit_code as u32) << 8,
        }
    }
    // todo:
}

bitflags! {
    pub struct WaitOptions: u32{
        const WNOHANG = wait::WNOHANG as u32;
        const WUNTRACED = wait::WUNTRACED as u32;
        const WCONTINUED = wait::WCONTINUED as u32;
    }
}

impl TryFromSyscallArg for WaitOptions {
    fn try_from_syscall_arg(value: u64) -> Result<Self, SysError> {
        WaitOptions::from_bits(value as u32).ok_or(SysError::InvalidArgument)
    }
}

#[syscall(SYS_WAIT4)]
pub fn sys_wait4(
    target: WaitObject,
    wstatus: Option<UserWritePtr<WStatus>>,
    waitoptions: WaitOptions,
) -> Result<u64, SysError> {
    if waitoptions.contains(WaitOptions::WUNTRACED) || waitoptions.contains(WaitOptions::WCONTINUED)
    {
        return Err(SysError::InvalidArgument);
        // unsupported
    }
    let task = unsafe {
        clone_current_task().waitpid(
            target,
            if waitoptions.contains(WaitOptions::WNOHANG) {
                false
            } else {
                true
            },
        )?
    };
    if let Some(task) = task {
        if let Some(wstatus) = wstatus {
            wstatus.safe_write(WStatus::normal(task.exit_code()))?;
        }
        Ok(task.tid().get() as u64)
    } else {
        Ok(0)
    }
}
