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

use crate::{prelude::*, task::Tid};

// Construction-time no-notify is fanotify-owned state keyed by the current
// task, with the value as a nesting depth for helper calls. It is not a fd
// table bit and must never decide behavior for already-returned files; those
// files use the generic OpenedFileDescriptionOps suppression marker instead.
static NO_NOTIFY_GUARDS: Lazy<Mutex<HashMap<Tid, usize>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Fanotify-local guard for the short window where read() reopens an event
/// object to manufacture the listener-visible fd.
///
/// It is deliberately separate from task/fd notification suppression: this
/// guard suppresses recursive enqueue during construction only, while the
/// returned event fd gets a generic opened-description marker for later I/O and
/// close paths. D5 hook code should consult the public facade below, not this
/// concrete guard type.
pub(super) struct NoNotifyGuard {
    tid: Tid,
}

impl NoNotifyGuard {
    pub(super) fn begin_current() -> Self {
        let tid = get_current_task().tid();
        let mut guards = NO_NOTIFY_GUARDS.lock();
        *guards.entry(tid).or_insert(0) += 1;
        Self { tid }
    }
}

impl Drop for NoNotifyGuard {
    fn drop(&mut self) {
        let mut guards = NO_NOTIFY_GUARDS.lock();
        let count = guards
            .get_mut(&self.tid)
            .expect("fanotify no-notify guard drop without active guard");
        // Underflow means the fanotify-local RAII protocol is broken; keep the
        // check as a correctness assertion because a leaked guard would
        // silently suppress future notifications for this task.
        assert!(*count > 0, "fanotify no-notify guard count underflow");
        *count -= 1;
        if *count == 0 {
            guards.remove(&self.tid);
        }
    }
}

/// Fanotify-local construction-time suppression state.
///
/// D4 only uses this guard around event-fd object open. D5 VFS hooks can query
/// this facade when they start enqueuing real events; returned event fds still
/// carry the generic opened-description notification-suppression marker.
#[allow(dead_code)]
pub fn current_task_notifications_suppressed() -> bool {
    let tid = get_current_task().tid();
    NO_NOTIFY_GUARDS
        .lock()
        .get(&tid)
        .is_some_and(|count| *count > 0)
}

// Gate A fixes the owner-module facade before VFS/registry/path-fd users are
// wired in. Keep these re-exports as the only module-external typed surface;
// later gates should consume them instead of reaching into fanotify internals.
#[allow(unused_imports)]
pub use api::{sys_fanotify_init, sys_fanotify_mark};
#[allow(unused_imports)]
pub use hooks::{FanHookEvent, notify_path_event};
#[allow(unused_imports)]
pub use types::FanMask;
