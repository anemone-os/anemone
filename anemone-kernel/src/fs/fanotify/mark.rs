//! Fanotify mark records and handles.
//!
//! The registry owns every `FanMarkRecord`. Target maps and group cleanup
//! lists only store `MarkHandle`, so masks, target references and target-dead
//! state have a single truth source.

use crate::prelude::*;

use super::{
    group::{FanGroup, FanGroupId},
    types::{FanMask, FanTarget, FanTargetKey},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MarkHandle {
    pub(super) group_id: FanGroupId,
    pub(super) group_generation: u64,
    pub(super) target_key: FanTargetKey,
    pub(super) slot: u64,
    pub(super) generation: u64,
}

impl MarkHandle {
    pub(super) const fn new(
        group_id: FanGroupId,
        group_generation: u64,
        target_key: FanTargetKey,
        slot: u64,
        generation: u64,
    ) -> Self {
        Self {
            group_id,
            group_generation,
            target_key,
            slot,
            generation,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FanMarkUpdate {
    pub(super) mask: FanMask,
    pub(super) ignored: bool,
    pub(super) ignored_survives_modify: bool,
}

impl FanMarkUpdate {
    pub(super) const fn event_mask(mask: FanMask) -> Self {
        Self {
            mask,
            ignored: false,
            ignored_survives_modify: false,
        }
    }

    pub(super) const fn ignored_mask(mask: FanMask, ignored_survives_modify: bool) -> Self {
        Self {
            mask,
            ignored: true,
            ignored_survives_modify,
        }
    }
}

#[derive(Debug)]
pub(super) struct FanMarkRecord {
    handle: MarkHandle,
    group: Weak<FanGroup>,
    // Keeps the watched object alive; registry identity uses handle.target_key.
    target: FanTarget,
    mask: FanMask,
    ignored_mask: FanMask,
    ignored_survives_modify: bool,
    target_dead: bool,
}

impl FanMarkRecord {
    pub(super) fn new(handle: MarkHandle, group: &Arc<FanGroup>, target: FanTarget) -> Self {
        assert!(
            handle.group_id == group.id() && handle.group_generation == group.generation(),
            "fanotify mark handle/group identity mismatch"
        );
        assert_eq!(
            handle.target_key,
            target.key(),
            "fanotify mark handle/target identity mismatch"
        );
        Self {
            handle,
            group: Arc::downgrade(group),
            target,
            mask: FanMask::empty(),
            ignored_mask: FanMask::empty(),
            ignored_survives_modify: false,
            target_dead: false,
        }
    }

    pub(super) const fn handle(&self) -> MarkHandle {
        self.handle
    }

    pub(super) const fn group_id(&self) -> FanGroupId {
        self.handle.group_id
    }

    pub(super) const fn group_generation(&self) -> u64 {
        self.handle.group_generation
    }

    pub(super) const fn target_key(&self) -> FanTargetKey {
        self.handle.target_key
    }

    pub(super) const fn mask(&self) -> FanMask {
        self.mask
    }

    pub(super) const fn ignored_mask(&self) -> FanMask {
        self.ignored_mask
    }

    pub(super) const fn ignored_survives_modify(&self) -> bool {
        self.ignored_survives_modify
    }

    pub(super) fn add_mask(&mut self, update: FanMarkUpdate) {
        if update.ignored {
            self.ignored_mask.insert(update.mask);
            if update.ignored_survives_modify {
                self.ignored_survives_modify = true;
            }
        } else {
            self.mask.insert(update.mask);
        }
    }

    pub(super) fn remove_mask(&mut self, update: FanMarkUpdate) {
        if update.ignored {
            self.ignored_mask.remove(update.mask);
            if self.ignored_mask.is_empty() {
                self.ignored_survives_modify = false;
            }
        } else {
            self.mask.remove(update.mask);
        }
    }

    pub(super) fn clear_ignored_mask_after_modify(&mut self) {
        if self.ignored_survives_modify {
            return;
        }
        self.ignored_mask = FanMask::empty();
        self.ignored_survives_modify = false;
    }

    pub(super) const fn is_empty(&self) -> bool {
        self.mask.is_empty() && self.ignored_mask.is_empty()
    }

    pub(super) const fn target_dead(&self) -> bool {
        self.target_dead
    }

    pub(super) fn mark_target_dead(&mut self) {
        self.target_dead = true;
    }

    pub(super) fn resolve_group(&self) -> Option<Arc<FanGroup>> {
        let group = self.group.upgrade()?;
        if group.id() != self.handle.group_id
            || group.generation() != self.handle.group_generation
            || group.is_dead()
        {
            return None;
        }
        Some(group)
    }
}
