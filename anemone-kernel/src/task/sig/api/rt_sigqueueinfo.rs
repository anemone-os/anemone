use crate::{
    prelude::*,
    syscall::user_access::UserReadPtr,
    task::sig::{
        Signal,
        info::{SiCode, SigInfoFields, SigRt},
    },
};

use anemone_abi::process::linux::signal as linux_signal;

use super::{KillSignal, check_send_kill_signal_permission, reject_kthread_task_signal_target};

/// Sends a queued signal with user-provided siginfo to a thread group.
///
/// Man page: https://www.man7.org/linux/man-pages/man2/rt_sigqueueinfo.2.html
///
/// Current permission check: sending to another thread group requires the
/// caller real/effective uid to match the target real/saved uid, or
/// `CAP_KILL`. `SIGCONT` is also allowed within the same session. Existing
/// siginfo validation remains unchanged.
#[syscall(SYS_RT_SIGQUEUEINFO)]
fn sys_rt_sigqueueinfo(pid: i32, sig: KillSignal, uinfo: u64) -> Result<u64, SysError> {
    kdebugln!(
        "sys_rt_sigqueueinfo: pid={}, sig={:?}, uinfo={:#x}",
        pid,
        sig,
        uinfo
    );

    let task = get_current_task();

    // rt_sigqueueinfo() resolves only a positive task id. Reject non-positive
    // pid_t values before converting to Anemone's unsigned Tid, otherwise they
    // become synthetic task ids and report ESRCH instead of EINVAL.
    if pid <= 0 {
        return Err(SysError::InvalidArgument);
    }
    let pid = Tid::new(pid as u32);

    // Linux signal 0 is a null-signal probe. It checks target existence and
    // permission, but it must not depend on caller-provided siginfo contents or
    // publish anything into pending signal queues.
    let signo = match sig {
        KillSignal::Probe => {
            let target = get_task(&pid).ok_or(SysError::NoSuchProcess)?;
            reject_kthread_task_signal_target(&target)?;
            check_send_kill_signal_permission(&target, sig)?;
            return Ok(0);
        },
        KillSignal::Armed(signo) => signo,
    };

    let kbuf = {
        let usp = task.clone_uspace_handle();
        let mut guard = usp.lock();
        let uinfo =
            UserReadPtr::<linux_signal::SigInfoWrapper>::try_new(VirtAddr::new(uinfo), &mut guard)?;
        uinfo.read()
    };

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
    reject_kthread_task_signal_target(&target)?;
    check_send_kill_signal_permission(&target, sig)?;
    let signal = Signal::new_with_errno(signo, si_code, si_fields, si_errno);
    target.get_thread_group().recv_signal(signal);

    Ok(0)
}
