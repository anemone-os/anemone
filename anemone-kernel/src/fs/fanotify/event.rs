use anemone_abi::fs::linux::fanotify as abi;

use crate::prelude::*;

use super::types::FanMask;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanEventKind {
    Synthetic,
    QueueOverflow,
}

#[derive(Debug, Clone)]
pub struct FanEvent {
    kind: FanEventKind,
    mask: FanMask,
    pid: i32,
}

impl FanEvent {
    pub fn synthetic(mask: FanMask) -> Self {
        Self {
            kind: FanEventKind::Synthetic,
            mask,
            pid: current_pid(),
        }
    }

    pub fn overflow() -> Self {
        Self {
            kind: FanEventKind::QueueOverflow,
            mask: FanMask::Q_OVERFLOW,
            pid: current_pid(),
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

    pub fn to_metadata(&self) -> abi::FanotifyEventMetadata {
        let metadata_len = abi::FAN_EVENT_METADATA_LEN;
        abi::FanotifyEventMetadata {
            event_len: metadata_len as u32,
            vers: abi::FANOTIFY_METADATA_VERSION,
            reserved: 0,
            metadata_len,
            mask: self.mask.bits(),
            fd: abi::FAN_NOFD,
            pid: self.pid,
        }
    }
}

fn current_pid() -> i32 {
    let raw = get_current_task().tid().get();
    i32::try_from(raw).unwrap_or(i32::MAX)
}
