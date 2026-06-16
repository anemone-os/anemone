//! signal-related system call and api
//!
//! References:
//! - https://www.man7.org/linux/man-pages/man2/rt_sigqueueinfo.2.html
//! - https://www.man7.org/linux/man-pages/man2/rt_sigaction.2.html
//! - https://www.man7.org/linux/man-pages/man2/rt_sigtimedwait.2.html
//! - https://www.man7.org/linux/man-pages/man2/rt_sigprocmask.2.html

use crate::{
    prelude::*,
    syscall::handler::TryFromSyscallArg,
    task::{ThreadGroup, ThreadGroupType, credentials::cap::Capability, sig::SigNo},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KillSignal {
    /// Linux treats signal 0 as a null signal: check target existence and
    /// permissions, but do not publish anything into pending signal queues.
    Probe,
    Armed(SigNo),
}

impl KillSignal {
    pub(super) const fn as_raw(self) -> usize {
        match self {
            Self::Probe => 0,
            Self::Armed(sig) => sig.as_usize(),
        }
    }
}

impl TryFromSyscallArg for KillSignal {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        if raw == 0 {
            return Ok(Self::Probe);
        }
        if raw >= anemone_abi::process::linux::signal::NSIG as u64 {
            return Err(SysError::InvalidArgument);
        }
        Ok(Self::Armed(SigNo::new(raw as usize)))
    }
}

macro_rules! deny_signal_permission {
    ($($arg:tt)*) => {{
        knoticeln!($($arg)*);
        SysError::PermissionDenied
    }};
}

fn can_send_signal_by_cred(target: &Arc<Task>) -> bool {
    let current = get_current_task();
    let current_cred = current.cred();
    let target_cred = target.cred();

    let uid_matches = current_cred.uid.real == target_cred.uid.real
        || current_cred.uid.real == target_cred.uid.saved
        || current_cred.uid.effective == target_cred.uid.real
        || current_cred.uid.effective == target_cred.uid.saved;
    uid_matches || current_cred.has_cap_effective(Capability::KILL)
}

pub(super) fn can_send_signal_to(target: &Arc<Task>, sig: SigNo) -> bool {
    if can_send_signal_by_cred(target) {
        return true;
    }

    let current = get_current_task();
    let current_tg = current.get_thread_group();
    let target_tg = target.get_thread_group();
    if target_tg.ty() != ThreadGroupType::User {
        return false;
    }
    if sig == SigNo::SIGCONT && current_tg.sid() == target_tg.sid() {
        return true;
    }

    false
}

pub(super) fn reject_kthread_signal_target(tg: &ThreadGroup) -> Result<(), SysError> {
    if tg.ty() == ThreadGroupType::KThread {
        return Err(SysError::NoSuchProcess);
    }
    Ok(())
}

pub(super) fn reject_kthread_task_signal_target(task: &Task) -> Result<(), SysError> {
    let tg = task.get_thread_group();
    reject_kthread_signal_target(&tg)
}

pub(super) fn can_send_kill_signal_to(target: &Arc<Task>, sig: KillSignal) -> bool {
    match sig {
        KillSignal::Probe => can_send_signal_by_cred(target),
        KillSignal::Armed(signo) => can_send_signal_to(target, signo),
    }
}

pub(super) fn check_send_kill_signal_permission(
    target: &Arc<Task>,
    sig: KillSignal,
) -> Result<(), SysError> {
    if can_send_kill_signal_to(target, sig) {
        return Ok(());
    }
    deny_send_signal_permission(target, sig.as_raw())
}

fn deny_send_signal_permission(target: &Arc<Task>, sig: usize) -> Result<(), SysError> {
    let current = get_current_task();
    let current_cred = current.cred();
    let target_cred = target.cred();

    Err(deny_signal_permission!(
        "signal denied: sig={}, sender uid/euid={}/{}, target uid/suid={}/{}, missing={:?}",
        sig,
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
