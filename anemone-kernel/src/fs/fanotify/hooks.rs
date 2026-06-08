//! VFS-facing fanotify hook facade.
//!
//! Gate A has no mark registry and no real VFS event injection. Hooks therefore
//! remain typed no-ops instead of exposing group, queue, or registry internals
//! to VFS callers. D5 will replace these with enqueue calls after D3/D4 close.

use super::types::FanMask;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanHookEvent {
    mask: FanMask,
}

impl FanHookEvent {
    pub const fn new(mask: FanMask) -> Self {
        Self { mask }
    }

    pub const fn mask(self) -> FanMask {
        self.mask
    }
}

pub fn notify_path_event(_event: FanHookEvent) {}
