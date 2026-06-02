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

use super::check_send_signal_permission;

#[syscall(SYS_TGKILL)]
fn sys_tgkill(tgid: Tid, tid: Tid, sig: SigNo) -> Result<u64, SysError> {
    kdebugln!(
        "sys_tgkill: tgid={}, tid={}, sig={}",
        tgid.get(),
        tid.get(),
        sig.as_usize(),
    );

    let current = get_current_task();
    let signal = Signal::new(
        sig,
        SiCode::TKill,
        SigInfoFields::TKill(SigKill {
            pid: current.tgid(),
            uid: current.cred().uid.real,
        }),
    );

    let tg = get_thread_group(&tgid).ok_or(SysError::NoSuchProcess)?;
    let thread = tg
        .find_member(|member| member.tid() == tid)
        .ok_or(SysError::NoSuchProcess)?;
    check_send_signal_permission(&thread, sig)?;
    thread.recv_signal(signal);

    Ok(0)
}
