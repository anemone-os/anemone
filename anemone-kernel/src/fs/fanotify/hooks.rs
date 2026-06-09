//! VFS-facing fanotify hook facade.
//!
//! VFS callers submit only event facts here. Matching, ignore-mask handling,
//! queueing, and group lifetime checks remain owned by `fs::fanotify`.

use crate::{
    prelude::*,
    task::files::{OpenedFileDescriptionOps, OpenedFileFinalReleaseCtx},
};

use super::{current_task_notifications_suppressed, registry, types::FanMask};

#[derive(Debug, Clone)]
pub struct FanHookEvent {
    mask: FanMask,
    path: PathRef,
    notification_suppressed: bool,
}

impl FanHookEvent {
    pub fn new(mask: FanMask, path: PathRef) -> Self {
        Self {
            mask,
            path,
            notification_suppressed: false,
        }
    }

    pub fn with_notification_suppressed(mut self, suppressed: bool) -> Self {
        self.notification_suppressed = suppressed;
        self
    }

    pub const fn mask(&self) -> FanMask {
        self.mask
    }

    pub fn path(&self) -> &PathRef {
        &self.path
    }

    pub(super) const fn notification_suppressed(&self) -> bool {
        self.notification_suppressed
    }

    pub(super) fn parent_path(&self) -> Option<PathRef> {
        self.path
            .dentry()
            .parent()
            .map(|parent| PathRef::new(self.path.mount().clone(), parent))
    }
}

pub fn notify_path_event(event: FanHookEvent) {
    if event.notification_suppressed() || current_task_notifications_suppressed() {
        return;
    }

    registry::notify_path_event(event);
}

pub fn observed_file_description_ops() -> OpenedFileDescriptionOps {
    // This is attached to ordinary opened file descriptions so FAN_CLOSE_*
    // comes from the final opened-description release. Fanotify group fds use
    // their own file ops and must not reuse this callback.
    OpenedFileDescriptionOps {
        final_release: Some(fanotify_observed_final_release),
        ..OpenedFileDescriptionOps::default()
    }
}

fn fanotify_observed_final_release(ctx: OpenedFileFinalReleaseCtx<'_>) {
    let mask = if ctx.access.can_write() {
        FanMask::CLOSE_WRITE
    } else {
        FanMask::CLOSE_NOWRITE
    };
    notify_path_event(
        FanHookEvent::new(mask, ctx.file.path().clone())
            .with_notification_suppressed(ctx.notification_suppressed),
    );
}
