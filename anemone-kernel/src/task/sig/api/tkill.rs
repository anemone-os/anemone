//! tkill system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/tkill.2.html

use crate::{
    prelude::*,
    task::sig::{
        SigNo, Signal,
        info::{SiCode, SigInfoFields, SigKill},
    },
};

use super::{KillSignal, check_send_kill_signal_permission, reject_kthread_task_signal_target};

/// Sends a signal to one specific task id.
///
/// Man page: https://www.man7.org/linux/man-pages/man2/tkill.2.html
///
/// Current permission check: sending to another thread group requires the
/// caller real/effective uid to match the target real/saved uid, or
/// `CAP_KILL`. `SIGCONT` is also allowed within the same session. Signal 0 is
/// a Linux null-signal probe: it checks target existence and permissions
/// without queueing or waking a signal.
#[syscall(SYS_TKILL)]
fn sys_tkill(tid: i32, sig: KillSignal) -> Result<u64, SysError> {
    kdebugln!("sys_tkill: tid={}, sig={:?}", tid, sig);

    if tid <= 0 {
        return Err(SysError::InvalidArgument);
    }
    let tid = Tid::new(tid as u32);

    let target = get_task(&tid).ok_or(SysError::NoSuchProcess)?;
    reject_kthread_task_signal_target(&target)?;
    check_send_kill_signal_permission(&target, sig)?;

    if let KillSignal::Armed(signo) = sig {
        target.recv_signal(tkill_signal(signo));
    }

    Ok(0)
}

fn tkill_signal(signo: SigNo) -> Signal {
    let current = get_current_task();
    Signal::new(
        signo,
        SiCode::TKill,
        SigInfoFields::TKill(SigKill {
            pid: current.tgid(),
            uid: current.cred().uid.real,
        }),
    )
}
