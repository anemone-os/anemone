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

// Gate A fixes the owner-module facade before VFS/registry/path-fd users are
// wired in. Keep these re-exports as the only module-external typed surface;
// later gates should consume them instead of reaching into fanotify internals.
#[allow(unused_imports)]
pub use api::{sys_fanotify_init, sys_fanotify_mark};
#[allow(unused_imports)]
pub use event::{FanEvent, FanEventKind};
#[allow(unused_imports)]
pub use group::{FanGroup, FanGroupId};
#[allow(unused_imports)]
pub use hooks::*;
#[allow(unused_imports)]
pub use types::{FanEventFdTemplate, FanGroupMode, FanInitFlags, FanMask};
