macro_rules! deny_permission {
    ($($arg:tt)*) => {{
        use crate::prelude::*;
        knoticeln!($($arg)*);
        SysError::PermissionDenied
    }};
}

pub mod cap;
pub mod gid;
pub mod groups;
pub mod id;
pub mod prctl;
pub mod uid;
