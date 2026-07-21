//! POSIX signals implementation.
//!
//! Note: we didn't take effort to abstract away from Linux's signal
//! implementation, since signal is a POSIX API, and we can't have 2 different
//! POSIX implementations in the same kernel. However this does not mean we
//! should copy Linux's implementation directly, we just follows similar design,
//! semantics, and the same uapi, to provide compatibility with Unix/Linux
//! user-space.
//!
//! Just a reminder: si_code, sa_flags and so on are not Linux-specific,
//! they are all defined in POSIX. So existence of [SiCode] or [SaFlags] doesn't
//! mean our kernel is polluted by Linux-internal stuff. But indeed we take the
//! same encoding as Linux for those fields for compatibility.
//!
//! Much logic relies on the fact that signal numbers are between 0 and 63. We
//! just hardcoded this fact in many places. We can refactor this later.

use anemone_abi::process::linux::{signal as linux_signal, signal::NSIG};

use crate::{
    prelude::*,
    syscall::handler::TryFromSyscallArg,
    task::sig::info::{SiCode, SigInfoFields},
};

mod api;
pub use api::*;
mod hal;
pub use hal::*;

pub mod altstack;
mod delivery;
pub(crate) use delivery::arbitrate_user_entry;
pub use delivery::{
    TemporaryMaskWaitCandidate, TemporaryMaskWaitContext, TemporaryMaskWaitDecision,
    TemporaryMaskWaitReturn, handle_signals,
};
pub mod disposition;
mod generation;
pub mod info;
mod mask;
pub use mask::{TaskSigMaskState, TemporarySigMaskToken};
mod pending;
pub use pending::PendingSignals;
pub mod set;

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
        SIGHUP, SIGINT, SIGQUIT, SIGILL, SIGTRAP, SIGABRT, SIGBUS, SIGFPE, SIGKILL, SIGUSR1,
        SIGSEGV, SIGUSR2, SIGPIPE, SIGALRM, SIGTERM, SIGCHLD, SIGCONT, SIGSTOP, SIGTSTP, SIGTTIN,
        SIGTTOU, SIGURG, SIGXCPU, SIGXFSZ, SIGVTALRM, SIGPROF, SIGWINCH, SIGIO, SIGPWR, SIGSYS
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

    /// Get the index of the realtime signal, if this is a realtime signal.
    pub const fn realtime_index(&self) -> Option<usize> {
        if self.is_realtime() {
            Some(self.as_usize() - SIGRTMIN as usize)
        } else {
            None
        }
    }
}

impl TryFromSyscallArg for SigNo {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        if raw == 0 || raw >= NSIG as u64 {
            return Err(SysError::InvalidArgument);
        }
        Ok(Self::new(raw as usize))
    }
}

/// A sent/sending signal.
#[derive(Debug, Clone)]
pub struct Signal {
    /// Signal number.
    no: SigNo,
    /// `si_errno` in `struct siginfo_t`.
    ///
    /// Linux almost always doesn't set this field, nor does user-space. We
    /// define it here for completeness.
    errno: i32,
    /// How the signal is generated.
    code: SiCode,
    /// Various information about the signal, depends on `code`.
    fields: SigInfoFields,
}

impl Signal {
    /// Create a new [Signal] with the given fields. `errno` is set to 0.
    ///
    /// Panics if the fields are not valid for the given code, which indicates a
    /// bug in kernel code.
    pub fn new(no: SigNo, code: SiCode, fields: SigInfoFields) -> Self {
        debug_assert!(fields.validate_with(code));
        Self {
            no,
            errno: 0,
            code,
            fields,
        }
    }

    /// Create a new [Signal] with the given fields and errno.
    ///
    /// Panics if the fields are not valid for the given code, which indicates a
    /// bug in kernel code.
    ///
    /// Why will you need to set errno? Idk. But let's just provide this API for
    /// completeness.
    pub fn new_with_errno(no: SigNo, code: SiCode, fields: SigInfoFields, errno: i32) -> Self {
        debug_assert!(fields.validate_with(code));
        Self {
            no,
            errno,
            code,
            fields,
        }
    }

    pub fn to_linux_siginfo(self) -> SigInfoWrapper {
        let mut kbuf = linux_signal::sifields::SigInfoFields::default();
        self.fields.serialize_to_linux(&mut kbuf);

        SigInfoWrapper {
            info: linux_signal::SigInfo {
                si_signo: self.no.as_usize() as i32,
                si_errno: self.errno,
                si_code: self.code.to_linux_code(),
                fields: kbuf,
            },
        }
    }
}
