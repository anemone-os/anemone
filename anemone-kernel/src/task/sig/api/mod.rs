//! signal-related system call and api
//!
//! References:
//! - https://www.man7.org/linux/man-pages/man2/rt_sigqueueinfo.2.html
//! - https://www.man7.org/linux/man-pages/man2/rt_sigaction.2.html
//! - https://www.man7.org/linux/man-pages/man2/rt_sigtimedwait.2.html
//! - https://www.man7.org/linux/man-pages/man2/rt_sigprocmask.2.html

use crate::{
    prelude::*,
    task::{credentials::cap::Capability, sig::SigNo},
};

macro_rules! deny_signal_permission {
    ($($arg:tt)*) => {{
        knoticeln!($($arg)*);
        SysError::PermissionDenied
    }};
}

pub(super) fn can_send_signal_to(target: &Arc<Task>, sig: SigNo) -> bool {
    let current = get_current_task();
    let current_cred = current.cred();
    let target_cred = target.cred();

    let uid_matches = current_cred.uid.real == target_cred.uid.real
        || current_cred.uid.real == target_cred.uid.saved
        || current_cred.uid.effective == target_cred.uid.real
        || current_cred.uid.effective == target_cred.uid.saved;
    if uid_matches || current_cred.has_cap_effective(Capability::KILL) {
        return true;
    }

    if sig == SigNo::SIGCONT && current.get_thread_group().sid() == target.get_thread_group().sid()
    {
        return true;
    }

    false
}

pub(super) fn check_send_signal_permission(target: &Arc<Task>, sig: SigNo) -> Result<(), SysError> {
    if can_send_signal_to(target, sig) {
        return Ok(());
    }

    let current = get_current_task();
    let current_cred = current.cred();
    let target_cred = target.cred();

    Err(deny_signal_permission!(
        "signal denied: sig={}, sender uid/euid={}/{}, target uid/suid={}/{}, missing={:?}",
        sig.as_usize(),
        current_cred.uid.real.get(),
        current_cred.uid.effective.get(),
        target_cred.uid.real.get(),
        target_cred.uid.saved.get(),
        Capability::KILL
    ))
}

pub mod kill;
pub mod rt_sigaction;
pub mod rt_sigpending;
pub mod rt_sigprocmask;
pub mod rt_sigqueueinfo;
pub mod rt_sigreturn;
pub mod rt_sigsuspend;
pub mod rt_sigtimedwait;
pub mod sigaltstack;
pub mod tgkill;
pub mod tkill;
