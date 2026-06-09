//! Fanotify mark registry owner.
//!
//! All ADD / REMOVE / FLUSH operations take this single registry lock. D5
//! matching must use the same lock before it snapshots matching records, so a
//! completed remove/flush cannot be observed by later matching.

use crate::prelude::*;

use super::{
    event::FanEvent,
    group::{FanGroup, FanGroupId},
    hooks::FanHookEvent,
    mark::{FanMarkRecord, FanMarkUpdate, MarkHandle},
    queue::FanDetachedTriggers,
    types::{FanMask, FanTarget, FanTargetClass, FanTargetKey},
};

static REGISTRY: Lazy<Mutex<FanRegistry>> = Lazy::new(|| Mutex::new(FanRegistry::new()));

#[derive(Debug)]
struct FanRegistry {
    next_slot: u64,
    records: HashMap<u64, FanMarkRecord>,
    marks_by_target: HashMap<FanTargetKey, Vec<MarkHandle>>,
}

impl FanRegistry {
    fn new() -> Self {
        Self {
            next_slot: 1,
            records: HashMap::new(),
            marks_by_target: HashMap::new(),
        }
    }

    fn next_handle(&mut self, group: &FanGroup, target_key: FanTargetKey) -> MarkHandle {
        let slot = self.next_slot;
        self.next_slot = self
            .next_slot
            .checked_add(1)
            .expect("fanotify mark slot id overflow");
        MarkHandle::new(group.id(), group.generation(), target_key, slot, 1)
    }

    fn find_handle_for_group(
        &self,
        target_key: FanTargetKey,
        group_id: FanGroupId,
        group_generation: u64,
    ) -> Option<MarkHandle> {
        self.marks_by_target
            .get(&target_key)?
            .iter()
            .copied()
            .find(|handle| {
                handle.group_id == group_id
                    && handle.group_generation == group_generation
                    && self.records.get(&handle.slot).is_some_and(|record| {
                        record.handle() == *handle && record.target_key() == target_key
                    })
            })
    }

    fn record_mut(&mut self, handle: MarkHandle) -> Option<&mut FanMarkRecord> {
        self.records
            .get_mut(&handle.slot)
            .filter(|record| record.handle() == handle)
    }

    fn remove_record_by_handle(&mut self, handle: MarkHandle) -> Option<FanMarkRecord> {
        let Some(record) = self.records.get(&handle.slot) else {
            return None;
        };
        if record.handle() != handle {
            return None;
        }

        let record = self.records.remove(&handle.slot)?;
        let target_key = handle.target_key;
        let remove_target_entry = if let Some(handles) = self.marks_by_target.get_mut(&target_key) {
            handles.retain(|existing| *existing != handle);
            handles.is_empty()
        } else {
            false
        };
        if remove_target_entry {
            self.marks_by_target.remove(&target_key);
        }
        Some(record)
    }

    fn remove_group_handle_for_record(record: &FanMarkRecord) {
        if let Some(group) = record.resolve_group() {
            group.remove_mark_handle_from_registry(record.handle());
        }
    }

    fn matching_handles(&self, target_key: FanTargetKey) -> Vec<MarkHandle> {
        self.marks_by_target
            .get(&target_key)
            .map(|handles| handles.to_vec())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FanMatchKind {
    SelfTarget,
    ParentChild,
}

#[derive(Debug)]
struct GroupMatch {
    group: Arc<FanGroup>,
    mask: FanMask,
    ignored_mask: FanMask,
}

impl GroupMatch {
    fn new(group: Arc<FanGroup>) -> Self {
        Self {
            group,
            mask: FanMask::empty(),
            ignored_mask: FanMask::empty(),
        }
    }
}

fn mark_applies_to_event(record_mask: FanMask, is_dir: bool, kind: FanMatchKind) -> bool {
    if is_dir && !record_mask.contains(FanMask::ONDIR) {
        return false;
    }
    if matches!(kind, FanMatchKind::ParentChild) && !record_mask.contains(FanMask::EVENT_ON_CHILD) {
        return false;
    }
    true
}

fn effective_legacy_ignored_mask(record: &FanMarkRecord, kind: FanMatchKind) -> FanMask {
    let ignored_mask = record.ignored_mask();
    if ignored_mask.is_empty() {
        return FanMask::empty();
    }

    match kind {
        FanMatchKind::SelfTarget => ignored_mask,
        // Current gate only accepts legacy FAN_MARK_IGNORED_MASK. Linux's
        // legacy mode ignores child events only when the ordinary mark mask
        // watches children; FAN_MARK_IGNORE's independent flag semantics stay
        // rejected by the parser.
        FanMatchKind::ParentChild if record.mask().contains(FanMask::EVENT_ON_CHILD) => {
            ignored_mask
        },
        FanMatchKind::ParentChild => FanMask::empty(),
    }
}

fn should_clear_legacy_ignore_after_modify(
    event_mask: FanMask,
    ignored_mask: FanMask,
    survives_modify: bool,
) -> bool {
    event_mask.contains(FanMask::MODIFY) && !ignored_mask.is_empty() && !survives_modify
}

fn add_group_match(
    matches: &mut Vec<GroupMatch>,
    group: Arc<FanGroup>,
    mask: FanMask,
    ignored_mask: FanMask,
) {
    if let Some(existing) = matches.iter_mut().find(|entry| {
        entry.group.id() == group.id() && entry.group.generation() == group.generation()
    }) {
        existing.mask.insert(mask);
        existing.ignored_mask.insert(ignored_mask);
        return;
    }

    let mut entry = GroupMatch::new(group);
    entry.mask.insert(mask);
    entry.ignored_mask.insert(ignored_mask);
    matches.push(entry);
}

fn collect_match_for_handle(
    registry: &mut FanRegistry,
    handle: MarkHandle,
    event_mask: FanMask,
    is_dir: bool,
    kind: FanMatchKind,
    matches: &mut Vec<GroupMatch>,
    remove_after_modify: &mut Vec<MarkHandle>,
) {
    let Some(record) = registry.record_mut(handle) else {
        return;
    };
    if record.target_dead() {
        return;
    }

    let record_mask = record.mask();
    let matched_mask = if mark_applies_to_event(record_mask, is_dir, kind) {
        record_mask
    } else {
        FanMask::empty()
    };
    let mut ignored_mask = effective_legacy_ignored_mask(record, kind);
    if should_clear_legacy_ignore_after_modify(
        event_mask,
        ignored_mask,
        record.ignored_survives_modify(),
    ) {
        // Linux clears non-surviving legacy ignore masks before deciding
        // whether the current modify event is ignored. Keep that ordering so
        // a FAN_MODIFY can both clear a legacy ignore mask and be delivered to
        // the same group when the ordinary mark mask is interested in it.
        record.clear_ignored_mask_after_modify();
        ignored_mask = FanMask::empty();
        if record.is_empty() {
            remove_after_modify.push(handle);
        }
    }
    if matched_mask.is_empty() && ignored_mask.is_empty() {
        return;
    }

    let Some(group) = record.resolve_group() else {
        return;
    };

    add_group_match(matches, group, matched_mask, ignored_mask);
}

fn collect_matches(
    registry: &mut FanRegistry,
    target_key: FanTargetKey,
    event_mask: FanMask,
    is_dir: bool,
    kind: FanMatchKind,
    matches: &mut Vec<GroupMatch>,
    remove_after_modify: &mut Vec<MarkHandle>,
) {
    for handle in registry.matching_handles(target_key) {
        collect_match_for_handle(
            registry,
            handle,
            event_mask,
            is_dir,
            kind,
            matches,
            remove_after_modify,
        );
    }
}

pub(super) fn notify_path_event(event: FanHookEvent) {
    let path = event.path().clone();
    let event_mask = event.mask();
    let is_dir = path.inode().ty() == InodeType::Dir;

    let target_inode = FanTargetKey::from_inode(path.inode());
    let target_mount = FanTargetKey::from_mount(path.mount());
    let target_sb = FanTargetKey::from_superblock(path.mount().sb());
    let parent_inode = event
        .parent_path()
        .map(|parent| FanTargetKey::from_inode(parent.inode()));

    let mut matches = Vec::new();
    {
        let mut registry = REGISTRY.lock();
        let mut remove_after_modify = Vec::new();

        collect_matches(
            &mut registry,
            target_inode,
            event_mask,
            is_dir,
            FanMatchKind::SelfTarget,
            &mut matches,
            &mut remove_after_modify,
        );
        collect_matches(
            &mut registry,
            target_mount,
            event_mask,
            is_dir,
            FanMatchKind::SelfTarget,
            &mut matches,
            &mut remove_after_modify,
        );
        collect_matches(
            &mut registry,
            target_sb,
            event_mask,
            is_dir,
            FanMatchKind::SelfTarget,
            &mut matches,
            &mut remove_after_modify,
        );
        if let Some(parent_inode) = parent_inode {
            collect_matches(
                &mut registry,
                parent_inode,
                event_mask,
                is_dir,
                FanMatchKind::ParentChild,
                &mut matches,
                &mut remove_after_modify,
            );
        }

        for handle in remove_after_modify {
            if let Some(record) = registry.remove_record_by_handle(handle) {
                FanRegistry::remove_group_handle_for_record(&record);
            }
        }
    }

    matches.sort_by_key(|entry| entry.group.id());
    for group_match in matches {
        // Current groups are legacy path-fd listeners. FAN_ONDIR and
        // FAN_EVENT_ON_CHILD decide whether a mark applies, but Linux does not
        // report those event-flag bits in metadata until FID mode is enabled.
        let delivered = event_mask & group_match.mask & !group_match.ignored_mask;
        if delivered.is_empty() {
            continue;
        }
        group_match
            .group
            .enqueue(FanEvent::path(delivered, path.clone()));
    }
}

pub(super) fn add_mark(
    group: &Arc<FanGroup>,
    target: FanTarget,
    update: FanMarkUpdate,
) -> Result<(), SysError> {
    let target_key = target.key();
    let mut registry = REGISTRY.lock();

    if group.is_dead() {
        return Err(SysError::InvalidArgument);
    }

    if let Some(handle) = registry.find_handle_for_group(target_key, group.id(), group.generation())
    {
        let record = registry
            .record_mut(handle)
            .expect("fanotify target map referenced missing mark record");
        record.add_mask(update);
        kdebugln!(
            "fanotify_mark: updated mark group={:?} target={:?} mask={:#x} ignored={}",
            group.id(),
            target_key,
            update.mask.bits(),
            update.ignored,
        );
        return Ok(());
    }

    let handle = registry.next_handle(group, target_key);
    let mut record = FanMarkRecord::new(handle, group, target);
    record.add_mask(update);

    registry.records.insert(handle.slot, record);
    registry
        .marks_by_target
        .entry(target_key)
        .or_default()
        .push(handle);

    if let Err(err) = group.add_mark_handle_from_registry(handle) {
        registry.remove_record_by_handle(handle);
        return Err(err);
    }

    kdebugln!(
        "fanotify_mark: added mark group={:?} target={:?} mask={:#x} ignored={}",
        group.id(),
        target_key,
        update.mask.bits(),
        update.ignored,
    );

    Ok(())
}

pub(super) fn remove_mark(
    group: &Arc<FanGroup>,
    target_key: FanTargetKey,
    update: FanMarkUpdate,
) -> Result<(), SysError> {
    let mut registry = REGISTRY.lock();
    let handle = registry
        .find_handle_for_group(target_key, group.id(), group.generation())
        .ok_or(SysError::NotFound)?;

    let destroy = {
        let record = registry
            .record_mut(handle)
            .expect("fanotify target map referenced missing mark record");
        record.remove_mask(update);
        record.is_empty()
    };

    if destroy {
        let record = registry
            .remove_record_by_handle(handle)
            .expect("fanotify mark disappeared before destroy");
        FanRegistry::remove_group_handle_for_record(&record);
    }

    kdebugln!(
        "fanotify_mark: removed mark bits group={:?} target={:?} mask={:#x} ignored={} destroy={}",
        group.id(),
        target_key,
        update.mask.bits(),
        update.ignored,
        destroy,
    );

    Ok(())
}

pub(super) fn flush_group(group: &Arc<FanGroup>, target_class: FanTargetClass) {
    let mut registry = REGISTRY.lock();
    let handles: Vec<_> = registry
        .records
        .values()
        .filter(|record| {
            record.group_id() == group.id()
                && record.group_generation() == group.generation()
                && record.target_key().class() == target_class
        })
        .map(FanMarkRecord::handle)
        .collect();

    for handle in &handles {
        if let Some(record) = registry.remove_record_by_handle(*handle) {
            FanRegistry::remove_group_handle_for_record(&record);
        }
    }

    kdebugln!(
        "fanotify_mark: flushed group={:?} target_class={:?} removed={}",
        group.id(),
        target_class,
        handles.len(),
    );
}

pub(super) fn mark_group_dead(group: &FanGroup) -> Option<FanDetachedTriggers> {
    {
        let mut registry = REGISTRY.lock();
        let handles = group.begin_mark_dead_from_registry()?;

        for handle in handles {
            registry.remove_record_by_handle(handle);
        }

        let stale_handles: Vec<_> = registry
            .records
            .values()
            .filter(|record| {
                record.group_id() == group.id() && record.group_generation() == group.generation()
            })
            .map(FanMarkRecord::handle)
            .collect();
        for handle in stale_handles {
            registry.remove_record_by_handle(handle);
        }
    }

    Some(group.clear_dead_queue_after_registry())
}
