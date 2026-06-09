use anemone_abi::fs::linux::fanotify as abi;

use crate::prelude::*;

use super::types::FanMask;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanEventKind {
    Synthetic,
    Path,
    QueueOverflow,
}

#[derive(Debug, Clone)]
enum FanEventTarget {
    // Synthetic and overflow events are pure metadata records. They must keep
    // fd == FAN_NOFD so Stage 1 behavior does not accidentally enter the D4
    // path-fd open/commit protocol.
    NoFd,
    // Path events hold a stable path snapshot until read() reopens the object
    // for the listener. The bounded queue owns this reference while queued.
    Path(PathRef),
}

#[derive(Debug, Clone)]
pub struct FanEvent {
    kind: FanEventKind,
    mask: FanMask,
    // FAN_REPORT_TID is rejected in the current gate, so metadata.pid follows
    // Linux's default fanotify ABI and reports the thread-group id. If TID
    // reporting is later enabled, make this a group report-mode decision.
    tgid: Tid,
    // This is the queued object identity, not a userspace fd identity. D4
    // turns it into metadata.fd only inside the read-user submit protocol, so
    // fanotify never stores a long-lived fd number outside the task/fd table.
    target: FanEventTarget,
}

impl FanEvent {
    pub fn synthetic(mask: FanMask) -> Self {
        Self {
            kind: FanEventKind::Synthetic,
            mask,
            tgid: current_task_id(),
            target: FanEventTarget::NoFd,
        }
    }

    pub fn path(mask: FanMask, path: PathRef) -> Self {
        // Path events are intentionally internal until real VFS hooks land.
        // Synthetic Stage 1 events still use FanEvent::synthetic() and cannot
        // accidentally allocate event fds during read().
        Self {
            kind: FanEventKind::Path,
            mask,
            tgid: current_task_id(),
            target: FanEventTarget::Path(path),
        }
    }

    pub fn overflow() -> Self {
        Self {
            kind: FanEventKind::QueueOverflow,
            mask: FanMask::Q_OVERFLOW,
            tgid: current_task_id(),
            target: FanEventTarget::NoFd,
        }
    }

    pub const fn kind(&self) -> FanEventKind {
        self.kind
    }

    pub const fn mask(&self) -> FanMask {
        self.mask
    }

    pub const fn metadata_len(&self) -> usize {
        abi::FAN_EVENT_METADATA_LEN as usize
    }

    pub fn path_target(&self) -> Option<&PathRef> {
        match &self.target {
            FanEventTarget::NoFd => None,
            FanEventTarget::Path(path) => Some(path),
        }
    }

    pub fn to_metadata_with_fd(&self, fd: i32) -> abi::FanotifyEventMetadata {
        let metadata_len = abi::FAN_EVENT_METADATA_LEN;
        abi::FanotifyEventMetadata {
            event_len: metadata_len as u32,
            vers: abi::FANOTIFY_METADATA_VERSION,
            reserved: 0,
            metadata_len,
            mask: self.mask.bits(),
            fd,
            pid: self.tgid.get() as i32,
        }
    }
}
