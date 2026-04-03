pub mod files;
#[path = "fs.rs"]
pub mod task_fs;
pub mod tid;

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

pub enum TaskError {
    ImageTooLarge,
}
