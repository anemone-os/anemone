pub mod cpu_usage;
pub mod files;
pub mod sig;
#[path = "fs.rs"]
pub mod task_fs;
pub mod tid;
pub mod wait;

mod api;
pub use api::*;
mod task;
pub use task::*;

/// Privilege Level of a control flow
#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Privilege {
    Kernel = 0,
    User = 1,
}

