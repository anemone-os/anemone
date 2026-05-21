pub mod getitimer;
pub mod setitimer;

use crate::prelude::*;

mod args {
    use crate::syscall::handler::TryFromSyscallArg;

    use super::*;

    #[derive(Debug)]
    pub enum ITimerWhich {
        Real,
        Virtual,
        Prof,
    }

    impl TryFromSyscallArg for ITimerWhich {
        fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
            use anemone_abi::time::linux::itimer::*;

            match raw as i32 {
                ITIMER_REAL => Ok(Self::Real),
                ITIMER_VIRTUAL => Ok(Self::Virtual),
                ITIMER_PROF => Ok(Self::Prof),
                _ => Err(SysError::InvalidArgument),
            }
        }
    }
}
use args::*;
