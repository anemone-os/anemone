//! getrlimit system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/getrlimit.2.html

use anemone_abi::process::linux::resource::RLimit;

use crate::syscall::user_access::{UserWritePtr, user_addr};

use super::*;

#[syscall(SYS_GETRLIMIT)]
fn sys_getrlimit(
    resource: RLimitResource,
    #[validate_with(user_addr)] rlim: VirtAddr,
) -> Result<u64, SysError> {
    kdebugln!("getrlimit: resource={:?}, rlim={:?}", resource, rlim);

    let task = get_current_task();
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

    let usp_handle = task.clone_uspace_handle();
    let mut usp = usp_handle.lock();

    UserWritePtr::<RLimit>::try_new(rlim, &mut usp)?.write(rlimit);

    Ok(0)
}
