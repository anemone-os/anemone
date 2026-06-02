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

use super::check_send_signal_permission;

/// Sends a signal to one specific task id.
///
/// Man page: https://www.man7.org/linux/man-pages/man2/tkill.2.html
///
/// Current permission check: sending to another thread group requires the
/// caller real/effective uid to match the target real/saved uid, or
/// `CAP_KILL`. `SIGCONT` is also allowed within the same session. The current
/// `SigNo` argument parser rejects signal 0 before this function is entered.
#[syscall(SYS_TKILL)]
fn sys_tkill(tid: Tid, sig: SigNo) -> Result<u64, SysError> {
    kdebugln!("sys_tkill: tid={}, sig={}", tid.get(), sig.as_usize(),);

    let current = get_current_task();
    let signal = Signal::new(
        sig,
        SiCode::TKill,
        SigInfoFields::TKill(SigKill {
            pid: current.tgid(),
            uid: current.cred().uid.real,
        }),
    );

    let target = get_task(&tid).ok_or(SysError::NoSuchProcess)?;
    check_send_signal_permission(&target, sig)?;
    target.recv_signal(signal);

    Ok(0)
}
