// TODO: better clone.
macro_rules! deny_permission {
    ($($arg:tt)*) => {{
        use crate::prelude::*;
        knoticeln!($($arg)*);
        SysError::PermissionDenied
    }};
}

pub mod clone;
pub mod execve;
pub mod exit;
pub mod futex;
pub mod getpid;
pub mod getppid;
pub mod gettid;
pub mod jobctl;
pub mod set_tid_address;
pub mod wait4;
