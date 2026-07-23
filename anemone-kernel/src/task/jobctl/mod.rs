//! ThreadGroup-owned Unix job-control state and mandatory user-entry gate.

mod api;
pub use api::*;

pub(super) mod group;
mod report;
pub(in crate::task) use report::ChildJobControlStatus;
mod terminal;
pub(crate) use terminal::{
    TtyCaller, TtyProcessGroup, TtySession, TtySessionLeader, TtySigttouDecision,
};
mod user_entry;
pub(in crate::task) use user_entry::UserEntryOutcome;
