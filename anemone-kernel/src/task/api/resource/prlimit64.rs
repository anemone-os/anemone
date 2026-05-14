//! prlimit64 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/prlimit64.2.html

use anemone_abi::process::linux::resource::RLimit;

use crate::{
    prelude::*,
    syscall::{
        handler::TryFromSyscallArg,
        user_access::{SyscallArgValidatorExt as _, UserWritePtr, user_addr},
    },
    task::task_resource::RLimitResource,
};

#[derive(Debug)]
enum PrLimitTarget {
    Celf,
    ThreadGroup(Tid),
}

impl TryFromSyscallArg for PrLimitTarget {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        match raw {
            0 => Ok(Self::Celf),
            tid => Ok(Self::ThreadGroup(Tid::try_from_syscall_arg(raw)?)),
        }
    }
}

#[syscall(SYS_PRLIMIT64)]
fn sys_prlimit64(
    target: PrLimitTarget,
    resource: RLimitResource,
    #[validate_with(user_addr.nullable())] new_limit: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] old_limit: Option<VirtAddr>,
) -> Result<u64, SysError> {
    kdebugln!(
        "prlimit64: target={:?}, resource={:?}, new_limit={:?}, old_limit={:?}",
        target,
        resource,
        new_limit,
        old_limit
    );

    // for now all thread groups share the same limits and we don't support changing
    // limits. so this is quite simplt.

    let task = get_current_task();
    let usp_handle = task.clone_uspace_handle();
    let mut usp = usp_handle.lock();

    if let Some(new_limit) = new_limit {
        knoticeln!("prlimit64: setting new limits is not supported, ignored.");
    }

    if let Some(old_limit) = old_limit {
        let rlimit = match resource {
            RLimitResource::Cpu => {
                RLimit {
                    rlim_cur: u64::MAX, // no CPU time limit
                    rlim_max: u64::MAX,
                }
            },
            RLimitResource::Fsize => {
                RLimit {
                    rlim_cur: u64::MAX, // no file size limit
                    rlim_max: u64::MAX,
                }
            },
            RLimitResource::NoFile => RLimit {
                rlim_cur: MAX_FD_PER_PROCESS as u64,
                rlim_max: MAX_FD_PER_PROCESS as u64,
            },
            RLimitResource::Stack => RLimit {
                rlim_cur: 1 << (USER_STACK_SHIFT_KB + 10),
                rlim_max: 1 << (USER_STACK_SHIFT_KB + 10),
            },
            r => {
                knoticeln!("getrlimit: unimplemented resource {:?}", r);
                return Err(SysError::NotYetImplemented);
            },
        };
        UserWritePtr::<RLimit>::try_new(old_limit, &mut usp)?.write(rlimit);
    }

    Ok(0)
}
