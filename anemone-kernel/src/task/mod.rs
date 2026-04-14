pub mod cpu_usage;
pub mod files;
pub mod sig;
#[path = "fs.rs"]
pub mod task_fs;
pub mod tid;
pub mod wait;

mod api;
use anemone_abi::errno;
pub use api::*;
mod task;
pub use task::*;

use crate::prelude::{AsErrno, SysError};

/// Privilege Level of a control flow
#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Privilege {
    Kernel = 0,
    User = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskError {
    ChildrenNotFound,
}

impl AsErrno for TaskError {
    fn as_errno(&self) -> anemone_abi::errno::Errno {
        match self {
            TaskError::ChildrenNotFound => errno::ECHILD,
        }
    }
}

impl Into<SysError> for TaskError {
    fn into(self) -> SysError {
        SysError::Task(self)
    }
}
