//! Typed POSIX/Linux siginfo_t with Rust enums and structs.

use crate::prelude::*;

/// `int si_code` in `struct siginfo_t`.
///
/// We adopts the same encoding as Linux for these codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiCode {
    Kernel,
    User,
    Queue,
    TKill,
    // TODO
}

impl SiCode {
    /// NYI code will be rejected.
    pub fn try_from_linux_code(code: i32) -> Result<Self, SysError> {
        use anemone_abi::process::linux::signal::*;
        match code {
            SI_KERNEL => Ok(Self::Kernel),
            SI_USER => Ok(Self::User),
            SI_QUEUE => Ok(Self::Queue),
            SI_TKILL => Ok(Self::TKill),
            _ => {
                knoticeln!("unrecognized si_code {} in siginfo_t", code);
                Err(SysError::InvalidArgument)
            },
        }
    }

    pub const fn to_linux_code(&self) -> i32 {
        use anemone_abi::process::linux::signal::*;
        match self {
            Self::Kernel => SI_KERNEL,
            Self::User => SI_USER,
            Self::Queue => SI_QUEUE,
            Self::TKill => SI_TKILL,
        }
    }

    pub const fn from_kernel(&self) -> bool {
        let code = self.to_linux_code();
        code > 0
    }

    pub const fn from_user(&self) -> bool {
        let code = self.to_linux_code();
        code <= 0
    }
}

/// Roughly typed version of Linux's `_sifields` in `struct siginfo_t`.
#[derive(Debug, Clone, Copy)]
pub enum SigInfoFields {
    Kill(SigKill),
    Rt(SigRt),
    Chld(SigChld),
    Fault(SigFault),
    TKill(SigKill),
    Ill(SigFault),
    // TODO: SigPoll, SigTimer, SigSys.
}

#[derive(Debug, Clone, Copy)]
pub struct SigKill {
    /// Sender's thread group ID.
    pub pid: Tid,
    /// Sender's user ID.
    pub uid: u32,
}

/// POSIX.1b signals
#[derive(Debug, Clone, Copy)]
pub struct SigRt {
    /// Sender's thread group ID.
    pub pid: Tid,
    /// Sender's user ID.
    pub uid: u32,
    /// Either an `i32` or a `pointer`. Anyway kernel doesn't care about
    /// that, so we just use `u64` here.
    pub sigval: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct SigChld {
    /// Child's thread group ID.
    pub pid: Tid,
    /// Sender's user ID.
    pub uid: u32,
    /// Exit code
    pub status: i32,
    /// User time consumed by the child.
    pub utime: u64,
    /// System time consumed by the child.
    pub stime: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct SigFault {
    pub addr: VirtAddr,
    // TODO: following is a huge union in Linux kernel. but they are seldom used. so we leave them
    // out for now.
}

impl SigInfoFields {
    /// Serialize typed [SigInfoFields] to untyped uapi `struct siginfo_t`.
    pub fn serialize_to_linux(
        &self,
        dst: &mut anemone_abi::process::linux::signal::sifields::SigInfoFields,
    ) {
        use anemone_abi::process::linux::signal::sifields::*;

        match self {
            Self::Kill(SigKill { pid, uid }) | Self::TKill(SigKill { pid, uid }) => {
                dst.kill = Kill {
                    pid: pid.get() as i32,
                    uid: *uid,
                }
            },
            Self::Rt(SigRt { pid, uid, sigval }) => {
                dst.rt = Rt {
                    pid: pid.get() as i32,
                    uid: *uid,
                    sigval: SigVal {
                        // just use the pointer field to store the sigval.
                        sival_ptr: *sigval as *mut _,
                    },
                }
            },
            Self::Chld(SigChld {
                pid,
                uid,
                status,
                utime,
                stime,
            }) => {
                dst.chld = Chld {
                    pid: pid.get() as i32,
                    uid: *uid,
                    status: *status,
                    utime: *utime,
                    stime: *stime,
                }
            },
            Self::Fault(SigFault { addr }) | Self::Ill(SigFault { addr }) => {
                dst.fault = Fault {
                    addr: addr.get() as usize as _, // TODO
                }
            },
        }
    }

    /// Validate if the fields are consistent with the given `si_code`.
    pub fn validate_with(&self, code: SiCode) -> bool {
        match (self, code) {
            (Self::Kill(_), SiCode::User | SiCode::Kernel) => true,
            (Self::Rt(_), SiCode::Queue) => true,
            (Self::Chld(_), SiCode::Kernel) => true,
            (Self::Fault(_), SiCode::Kernel) => true,
            (Self::Ill(_), SiCode::Kernel) => true,
            (Self::TKill(_), SiCode::TKill) => true,
            _ => {
                kdebugln!(
                    "invalid combination of si_code {:?} and siginfo fields {:?}",
                    code,
                    self
                );
                false
            },
        }
    }
}
