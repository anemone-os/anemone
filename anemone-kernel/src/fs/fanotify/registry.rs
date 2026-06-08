//! Fanotify mark registry owner.
//!
//! All ADD / REMOVE / FLUSH operations take this single registry lock. D5
//! matching must use the same lock before it snapshots matching records, so a
//! completed remove/flush cannot be observed by later matching.

use crate::prelude::*;

use super::{
    group::{FanGroup, FanGroupId},
    mark::{FanMarkRecord, FanMarkUpdate, MarkHandle},
    queue::FanDetachedTriggers,
    types::{FanTarget, FanTargetClass, FanTargetKey},
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
        let remove_target_entry = if let Some(handles) = self.marks_by_target.get_mut(&target_key)
        {
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

    if let Some(handle) =
        registry.find_handle_for_group(target_key, group.id(), group.generation())
    {
        let record = registry
            .record_mut(handle)
            .expect("fanotify target map referenced missing mark record");
        record.add_mask(update);
        kdebugln!(
            "fanotify_mark: updated mark group={:?} target={:?} mask={:#x} ignored={}",
            group.id(),
            target_key,
            update.mask().bits(),
            update.ignored(),
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
        update.mask().bits(),
        update.ignored(),
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
        update.mask().bits(),
        update.ignored(),
        destroy,
    );

    Ok(())
}

pub(super) fn flush_group(
    group: &Arc<FanGroup>,
    target_class: FanTargetClass,
) -> Result<(), SysError> {
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

    Ok(())
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
