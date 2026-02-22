#[macro_use]
pub mod int_like;
#[macro_use]
pub mod align;
#[macro_use]
pub mod macros;

pub mod writer;

pub mod circular_log;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Todo;
