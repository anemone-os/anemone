use anemone_abi::syscall::SYS_EPOLL_PWAIT;

use crate::{
    prelude::*,
    syscall::user_access::{SyscallArgValidatorExt as _, user_addr},
    task::files::Fd,
};

use super::wait::run_epoll_wait;

#[syscall(SYS_EPOLL_PWAIT)]
fn sys_epoll_pwait(
    epfd: Fd,
    #[validate_with(user_addr.nullable())] events_addr: Option<VirtAddr>,
    maxevents: i32,
    timeout_ms: i32,
    #[validate_with(user_addr.nullable())] sigmask_addr: Option<VirtAddr>,
    sigsetsize: usize,
) -> Result<u64, SysError> {
    let timeout = (timeout_ms >= 0).then(|| Duration::from_millis(timeout_ms as u64));
    run_epoll_wait(
        "sys_epoll_pwait",
        epfd,
        events_addr,
        maxevents,
        timeout,
        sigmask_addr,
        sigsetsize,
    )
}
