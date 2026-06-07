use crate::{
    prelude::*,
    syscall::user_access::{UserReadPtr, user_addr},
    task::sig::{
        Signal,
        info::{SiCode, SigInfoFields, SigRt},
    },
};

use anemone_abi::process::linux::signal as linux_signal;

use super::{KillSignal, check_send_kill_signal_permission};

/// Sends a queued signal with user-provided siginfo to a thread group.
///
/// Man page: https://www.man7.org/linux/man-pages/man2/rt_sigqueueinfo.2.html
///
/// Current permission check: sending to another thread group requires the
/// caller real/effective uid to match the target real/saved uid, or
/// `CAP_KILL`. `SIGCONT` is also allowed within the same session. Existing
/// siginfo validation remains unchanged.
#[syscall(SYS_RT_SIGQUEUEINFO)]
fn sys_rt_sigqueueinfo(
    pid: i32,
    sig: KillSignal,
    #[validate_with(user_addr)] uinfo: VirtAddr,
) -> Result<u64, SysError> {
    kdebugln!(
        "sys_rt_sigqueueinfo: pid={}, sig={:?}, uinfo={:?}",
        pid,
        sig,
        uinfo
    );

    let task = get_current_task();
    let pid = Tid::new(pid as u32);

    let mut kbuf = linux_signal::SigInfoWrapper::default();

    {
        let usp = task.clone_uspace_handle();
        let mut guard = usp.lock();
        let uinfo = UserReadPtr::<linux_signal::SigInfoWrapper>::try_new(uinfo, &mut guard)?;
        kbuf = uinfo.read();
    }

    // parse kbuf to our internal data structure.

    let (si_code, si_errno, sifields) =
        unsafe { (kbuf.info.si_code, kbuf.info.si_errno, kbuf.info.fields) };

    let si_code = SiCode::try_from_linux_code(si_code)?;
    if let SiCode::Kernel = si_code {
        return Err(SysError::InvalidArgument);
    }

    let si_fields = unsafe {
        SigInfoFields::Rt(SigRt {
            pid: task.tgid(),
            uid: task.cred().uid.real,
            sigval: sifields.rt.sigval.as_u64(),
        })
    };

    // Linux rt_sigqueueinfo() first resolves pid as PIDTYPE_PID, then sends a
    // process-directed signal to the resolved task's thread group. This matters
    // for callers that pass a non-leader gettid(): lookup must succeed even
    // though delivery remains on the shared pending queue.
    let target = get_task(&pid).ok_or(SysError::NoSuchProcess)?;
    check_send_kill_signal_permission(&target, sig)?;
    if let KillSignal::Armed(signo) = sig {
        let signal = Signal::new_with_errno(signo, si_code, si_fields, si_errno);
        target.get_thread_group().recv_signal(signal);
    }

    Ok(0)
}
