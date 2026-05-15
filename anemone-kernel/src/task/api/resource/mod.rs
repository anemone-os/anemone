#[cfg(target_arch = "riscv64")]
pub mod getrlimit;
pub mod getrusage;
pub mod prlimit64;

use crate::{prelude::*, syscall::handler::TryFromSyscallArg};

#[derive(Debug)]
enum RLimitResource {
    Cpu,
    Fsize,
    Data,
    Stack,
    Core,
    Rss,
    Nproc,
    NoFile,
    Memlock,
    As,
    Locks,
    Sigpending,
    Msgqueue,
    Nice,
    Rtprio,
    Rttime,
}

impl TryFromSyscallArg for RLimitResource {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        use anemone_abi::process::linux::resource::*;

        match raw as u32 {
            RLIMIT_CPU => Ok(Self::Cpu),
            RLIMIT_FSIZE => Ok(Self::Fsize),
            RLIMIT_DATA => Ok(Self::Data),
            RLIMIT_STACK => Ok(Self::Stack),
            RLIMIT_CORE => Ok(Self::Core),
            RLIMIT_RSS => Ok(Self::Rss),
            RLIMIT_NPROC => Ok(Self::Nproc),
            RLIMIT_NOFILE => Ok(Self::NoFile),
            RLIMIT_MEMLOCK => Ok(Self::Memlock),
            RLIMIT_AS => Ok(Self::As),
            RLIMIT_LOCKS => Ok(Self::Locks),
            RLIMIT_SIGPENDING => Ok(Self::Sigpending),
            RLIMIT_MSGQUEUE => Ok(Self::Msgqueue),
            RLIMIT_NICE => Ok(Self::Nice),
            RLIMIT_RTPRIO => Ok(Self::Rtprio),
            RLIMIT_RTTIME => Ok(Self::Rttime),
            _ => {
                knoticeln!("getrlimit: unknown resource ID {:#x}", raw);
                Err(SysError::InvalidArgument)
            },
        }
    }
}
