mod task;
pub use task::*;
pub mod tid;
mod api;
pub use api::*;

/// Privilege Level of a control flow
#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Privilege {
    Kernel = 0,
    User = 1,
}
