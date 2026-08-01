use anemone_abi::{syscall::SYS_EPOLL_PWAIT2, time::linux::TimeSpec};

use crate::{
    prelude::*,
    syscall::user_access::{SyscallArgValidatorExt as _, UserReadPtr, user_addr},
    task::files::Fd,
};

use super::wait::run_epoll_wait;

#[syscall(SYS_EPOLL_PWAIT2)]
fn sys_epoll_pwait2(
    epfd: Fd,
    #[validate_with(user_addr.nullable())] events_addr: Option<VirtAddr>,
    maxevents: i32,
    #[validate_with(user_addr.nullable())] timeout_addr: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] sigmask_addr: Option<VirtAddr>,
    sigsetsize: usize,
) -> Result<u64, SysError> {
    let timeout = timeout_addr
        .map(|timeout_addr| {
            let task = get_current_task();
            let usp_handle = task.clone_uspace_handle();
            let mut usp = usp_handle.lock();
            let TimeSpec { tv_sec, tv_nsec } =
                UserReadPtr::<TimeSpec>::try_new(timeout_addr, &mut usp)?.read()?;
            if tv_sec < 0 || tv_nsec < 0 || tv_nsec >= 1_000_000_000 {
                return Err(SysError::InvalidArgument);
            }
            Ok(Duration::new(tv_sec as u64, tv_nsec as u32))
        })
        .transpose()?;

    run_epoll_wait(
        "sys_epoll_pwait2",
        epfd,
        events_addr,
        maxevents,
        timeout,
        sigmask_addr,
        sigsetsize,
    )
}
