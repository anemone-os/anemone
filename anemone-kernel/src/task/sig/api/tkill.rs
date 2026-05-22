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

#[syscall(SYS_TKILL)]
fn sys_tkill(tid: Tid, sig: SigNo) -> Result<u64, SysError> {
    kdebugln!("sys_tkill: tid={}, sig={}", tid.get(), sig.as_usize(),);

    let signal = Signal::new(
        sig,
        SiCode::TKill,
        SigInfoFields::TKill(SigKill {
            pid: get_current_task().tgid(),
            uid: 0,
        }),
    );

    get_task(&tid)
        .ok_or(SysError::NoSuchProcess)?
        .recv_signal(signal);

    Ok(0)
}
