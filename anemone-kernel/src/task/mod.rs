mod task;
pub use task::*;
pub mod tid;

/// Privilege Level of a control flow
#[repr(u64)]
pub enum Privilege {
    Kernel = 0,
    User = 1,
}
