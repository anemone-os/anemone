//! Fanotify owner module.
//!
//! This module owns fanotify group state, queue state, file private data,
//! syscall parsing, and future mark registry/matching state. Code outside this
//! directory must use the typed facade here instead of downcasting group fd
//! private data.

mod api;
mod event;
mod file;
mod group;
mod hooks;
mod mark;
mod queue;
mod registry;
mod types;

pub use api::*;
pub use hooks::{
    FanHookEvent, notify_opened_file_event, notify_path_event, observed_file_description_ops,
};
pub use types::FanMask;
