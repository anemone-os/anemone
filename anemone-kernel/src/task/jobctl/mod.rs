//! ThreadGroup-owned Unix job-control state and mandatory user-entry gate.

mod api;
pub use api::*;

pub(super) mod group;
mod user_entry;
