//! tgkill system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/tgkill.2.html

use crate::{
    prelude::*,
    task::sig::{
        SigNo, Signal,
        info::{SiCode, SigInfoFields, SigKill},
    },
};

use super::{
    KillSignal, check_send_kill_signal_permission, reject_kthread_signal_target,
    reject_kthread_task_signal_target,
};

#[syscall(SYS_TGKILL)]
fn sys_tgkill(tgid: i32, tid: i32, sig: KillSignal) -> Result<u64, SysError> {
    kdebugln!("sys_tgkill: tgid={}, tid={}, sig={:?}", tgid, tid, sig);

    // Linux tgkill(2) takes pid_t arguments and rejects non-positive task
    // identities before task lookup. Keep that check before converting into
    // Anemone's unsigned Tid, otherwise negative pid_t values would turn into
    // large synthetic TIDs and report ESRCH instead of EINVAL.
    if tgid <= 0 || tid <= 0 {
        return Err(SysError::InvalidArgument);
    }
    let tgid = Tid::new(tgid as u32);
    let tid = Tid::new(tid as u32);

    let tg = get_thread_group(&tgid).ok_or(SysError::NoSuchProcess)?;
    reject_kthread_signal_target(&tg)?;
    let thread = tg
        .find_member(|member| member.tid() == tid)
        .ok_or(SysError::NoSuchProcess)?;
    reject_kthread_task_signal_target(&thread)?;
    check_send_kill_signal_permission(&thread, sig)?;
    if let KillSignal::Armed(signo) = sig {
        thread.recv_signal(tkill_signal(signo));
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
