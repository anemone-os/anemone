use super::*;

use crate::sys::linux::process::signal;
use anemone_abi::time::linux::TimeSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SigNo(usize);

macro_rules! define_typed_signo {
    ($($no:ident),*) => {
        $(
            pub const $no: Self = Self($no as usize);
        )*
    };
}
use anemone_abi::process::linux::signal::*;
impl SigNo {
    define_typed_signo!(
        SIGHUP, SIGINT, SIGQUIT, SIGILL, SIGTRAP, SIGABRT, SIGBUS, SIGFPE, SIGKILL,
        SIGUSR1, SIGSEGV, SIGUSR2, SIGPIPE, SIGALRM, SIGTERM, SIGCHLD, SIGCONT, SIGSTOP,
        SIGTSTP, SIGTTIN, SIGTTOU, SIGURG, SIGXCPU, SIGXFSZ, SIGVTALRM, SIGPROF, SIGWINCH,
        SIGIO, SIGPWR, SIGSYS
    );
}

impl SigNo {
    pub fn new(sig: usize) -> Self {
        assert!(
            sig < NSIG && sig != 0,
            "signal number {} is out of range",
            sig
        );
        Self(sig)
    }

    pub const fn as_usize(&self) -> usize {
        self.0
    }

    pub const fn is_realtime(&self) -> bool {
        self.as_usize() >= SIGRTMIN as usize && self.as_usize() <= SIGRTMAX as usize
    }

    pub const fn is_unreliable(&self) -> bool {
        !self.is_realtime()
    }

    /// Get the index of the realtime signal, if this is a realtime
    /// signal.
    pub const fn realtime_index(&self) -> Option<usize> {
        if self.is_realtime() {
            Some(self.as_usize() - SIGRTMIN as usize)
        } else {
            None
        }
    }
}

pub fn sigaction(
    sig: SigNo,
    act: Option<&SigAction>,
    oldact: Option<&mut SigAction>,
) -> Result<(), Errno> {
    signal::rt_sigaction(
        sig.as_usize() as u64,
        act.map_or(0, |a| a as *const SigAction as u64),
        oldact
            .and_then(|o| Some(o as *mut SigAction as u64))
            .unwrap_or(0),
        size_of::<SigSet>() as u64,
    )
    .map(|_| ())
}

pub fn kill(pid: i32, sig: SigNo) -> Result<(), Errno> {
    signal::kill(pid, sig.as_usize() as u32).map(|_| ())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigProcMaskHow {
    Block,
    Unblock,
    SetMask,
}

impl SigProcMaskHow {
    pub fn to_raw(&self) -> i32 {
        match self {
            SigProcMaskHow::Block => SIG_BLOCK,
            SigProcMaskHow::Unblock => SIG_UNBLOCK,
            SigProcMaskHow::SetMask => SIG_SETMASK,
        }
    }
}

pub fn sigprocmask(
    how: SigProcMaskHow,
    set: Option<&SigSet>,
    oldset: Option<&mut SigSet>,
) -> Result<(), Errno> {
    signal::rt_sigprocmask(
        how.to_raw() as u64,
        set.map_or(0, |s| s as *const SigSet as u64),
        oldset
            .and_then(|o| Some(o as *mut SigSet as u64))
            .unwrap_or(0),
        size_of::<SigSet>() as u64,
    )
    .map(|_| ())
}

pub fn sigaltstack(
    uss: Option<&SigStack>,
    uoss: Option<&mut SigStack>,
) -> Result<(), Errno> {
    signal::sigaltstack(
        uss.map_or(0, |s| s as *const SigStack as u64),
        uoss.and_then(|o| Some(o as *mut SigStack as u64))
            .unwrap_or(0),
    )
    .map(|_| ())
}

pub fn tgkill(tgid: Tid, tid: Tid, sig: SigNo) -> Result<(), Errno> {
    signal::tgkill(tgid as u64, tid as u64, sig.as_usize() as u64).map(|_| ())
}

pub fn tkill(tid: Tid, sig: SigNo) -> Result<(), Errno> {
    signal::tkill(tid as u64, sig.as_usize() as u64).map(|_| ())
}

pub fn sigqueueinfo(pid: Tid, sig: SigNo, siginfo: &SigInfoWrapper) -> Result<(), Errno> {
    signal::rt_sigqueueinfo(
        pid as u64,
        sig.as_usize() as u64,
        siginfo as *const SigInfoWrapper as u64,
    )
    .map(|_| ())
}

pub fn raise(sig: SigNo) -> Result<(), Errno> {
    let tid = gettid()?;
    let tgid = getpid()?;
    tgkill(tgid, tid, sig)
}

pub fn sigreturn() -> Result<(), Errno> {
    signal::rt_sigreturn().map(|_| ())
}

pub fn rt_sigtimedwait(
    set: &SigSet,
    timeout: Option<&TimeSpec>,
) -> Result<SigInfo, Errno> {
    // The kernel fills the complete Linux siginfo wire frame; expose the
    // typed value only after the syscall has completed and copied it out.
    let mut info = SigInfoWrapper::default();
    signal::rt_sigtimedwait(
        set as *const SigSet as u64,
        &mut info as *mut SigInfoWrapper as u64,
        timeout.map_or(0, |timeout| timeout as *const TimeSpec as u64),
        size_of::<SigSet>() as u64,
    )?;
    Ok(unsafe { info.info })
}
