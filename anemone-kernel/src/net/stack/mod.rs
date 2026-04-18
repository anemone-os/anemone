//! Physical device bridge and [`NetStack`] construction.

mod adapter;
mod build;
mod netstack;

pub(crate) use build::build_stack;
pub(crate) use netstack::NetStack;
