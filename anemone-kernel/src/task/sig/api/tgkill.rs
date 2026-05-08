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

#[syscall(SYS_TGKILL)]
fn sys_tgkill(tgid: Tid, tid: Tid, sig: SigNo) -> Result<u64, SysError> {
    kdebugln!(
        "sys_tgkill: tgid={}, tid={}, sig={}",
        tgid.get(),
        tid.get(),
        sig.as_usize(),
    );

    let signal = Signal::new(
        sig,
        SiCode::TKill,
        SigInfoFields::TKill(SigKill {
            pid: get_current_task().tgid(),
            uid: 0,
        }),
    );

    let tg = get_thread_group(&tgid).ok_or(SysError::NoSuchProcess)?;
    let thread = tg
        .find_member(|member| member.tid() == tid)
        .ok_or(SysError::NoSuchProcess)?;
    thread.recv_signal(signal);

    Ok(0)
}
