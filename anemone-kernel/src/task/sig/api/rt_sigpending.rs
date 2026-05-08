use crate::{
    prelude::*,
    syscall::user_access::{UserWritePtr, user_addr},
};

use anemone_abi::process::linux::signal as linux_signal;

#[syscall(SYS_RT_SIGPENDING)]
fn sys_rt_sigpending(
    #[validate_with(user_addr)] uset: VirtAddr,
    sigsetsize: usize,
) -> Result<u64, SysError> {
    kdebugln!(
        "sys_rt_sigpending: uset={:?}, sigsetsize={}",
        uset,
        sigsetsize
    );

    if sigsetsize != size_of::<linux_signal::SigSet>() {
        knoticeln!("sys_rt_sigpending: invalid sigsetsize: {}", sigsetsize);
        return Err(SysError::InvalidArgument);
    }

    let (usp, set) = {
        let task = get_current_task();
        let mut kbuf = linux_signal::SigSet { bits: 0 };

        let prv_set = task.sig_pending.lock().to_sigset();
        let shared_set = task
            .get_thread_group()
            .inner
            .read()
            .sig_pending
            .lock()
            .to_sigset();

        kbuf.bits = (prv_set.union(&shared_set)).as_u64();

        (task.clone_uspace(), kbuf)
    };

    {
        let mut guard = usp.write();
        UserWritePtr::<linux_signal::SigSet>::try_new(uset, &mut guard)?.write(set);
    }

    Ok(0)
}
