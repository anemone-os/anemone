use core::cmp::Ordering;

use crate::{
    prelude::*,
    task::files::{PosixLockBinding, PosixLockHolder},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PosixLockRange {
    start: u64,
    end_exclusive: Option<u64>,
}

impl PosixLockRange {
    pub(crate) fn finite(start: u64, end_exclusive: u64) -> Self {
        assert!(start < end_exclusive, "POSIX lock range must be nonempty");
        Self {
            start,
            end_exclusive: Some(end_exclusive),
        }
    }

    pub(crate) const fn open_ended(start: u64) -> Self {
        Self {
            start,
            end_exclusive: None,
        }
    }

    fn starts_before_end(start: u64, end_exclusive: Option<u64>) -> bool {
        end_exclusive.is_none_or(|end| start < end)
    }

    fn overlaps(self, other: Self) -> bool {
        Self::starts_before_end(self.start, other.end_exclusive)
            && Self::starts_before_end(other.start, self.end_exclusive)
    }

    fn touches_or_overlaps(self, next: Self) -> bool {
        assert!(self.start <= next.start);
        self.end_exclusive.is_none_or(|end| end >= next.start)
    }

    fn merge(self, next: Self) -> Self {
        assert!(self.touches_or_overlaps(next));
        let end_exclusive = match (self.end_exclusive, next.end_exclusive) {
            (None, _) | (_, None) => None,
            (Some(left), Some(right)) => Some(left.max(right)),
        };
        Self {
            start: self.start,
            end_exclusive,
        }
    }

    fn outside_parts(self, assigned: Self) -> (Option<Self>, Option<Self>) {
        assert!(self.overlaps(assigned));

        let prefix =
            (self.start < assigned.start).then(|| Self::finite(self.start, assigned.start));
        let suffix = match assigned.end_exclusive {
            Some(assigned_end) if Self::starts_before_end(assigned_end, self.end_exclusive) => {
                Some(match self.end_exclusive {
                    Some(end) => Self::finite(assigned_end, end),
                    None => Self::open_ended(assigned_end),
                })
            },
            _ => None,
        };
        (prefix, suffix)
    }

    fn cmp(self, other: Self) -> Ordering {
        self.start
            .cmp(&other.start)
            .then_with(|| match (self.end_exclusive, other.end_exclusive) {
                (Some(left), Some(right)) => left.cmp(&right),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            })
    }

    pub(crate) const fn start(self) -> u64 {
        self.start
    }

    pub(crate) const fn end_exclusive(self) -> Option<u64> {
        self.end_exclusive
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PosixLockMode {
    Read,
    Write,
}

impl PosixLockMode {
    fn conflicts_with(self, requested: Self) -> bool {
        self == Self::Write || requested == Self::Write
    }
}

#[derive(Debug, Clone)]
struct PosixLockSegment {
    owner: PosixLockHolder,
    range: PosixLockRange,
    mode: PosixLockMode,
    /// Diagnostic snapshot only: it may become stale and never participates
    /// in owner identity, conflict, canonicalization, or lifecycle decisions.
    report_tgid: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PosixLockConflict {
    range: PosixLockRange,
    mode: PosixLockMode,
    report_tgid: u32,
}

impl From<&PosixLockSegment> for PosixLockConflict {
    fn from(segment: &PosixLockSegment) -> Self {
        Self {
            range: segment.range,
            mode: segment.mode,
            report_tgid: segment.report_tgid,
        }
    }
}

impl PosixLockConflict {
    pub(crate) const fn range(self) -> PosixLockRange {
        self.range
    }

    pub(crate) const fn mode(self) -> PosixLockMode {
        self.mode
    }

    pub(crate) const fn report_tgid(self) -> u32 {
        self.report_tgid
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PosixLockQueryOutcome {
    Available,
    Conflict(PosixLockConflict),
    BindingRetired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PosixLockSetOutcome {
    Applied,
    Conflict(PosixLockConflict),
    BindingRetired,
    Interrupted,
}

#[derive(Debug)]
pub(in crate::fs) struct PosixLockDomain {
    /// Sole persistent truth for POSIX holder-to-range grants on this inode.
    segments: SpinLock<Vec<PosixLockSegment>>,
    /// Notification only. Grant and waiter eligibility remain derivable from
    /// `segments` plus the operation-local binding liveness capability.
    recheck: Event,
}

impl PosixLockDomain {
    pub(in crate::fs) const fn new() -> Self {
        Self {
            segments: SpinLock::new(Vec::new()),
            recheck: Event::new(),
        }
    }

    fn find_conflict(
        segments: &[PosixLockSegment],
        owner: &PosixLockHolder,
        range: PosixLockRange,
        mode: PosixLockMode,
    ) -> Option<PosixLockConflict> {
        segments
            .iter()
            .find(|segment| {
                !segment.owner.same_identity(owner)
                    && segment.range.overlaps(range)
                    && segment.mode.conflicts_with(mode)
            })
            .map(PosixLockConflict::from)
    }

    fn canonicalize_owner(mut segments: Vec<PosixLockSegment>) -> Vec<PosixLockSegment> {
        segments.sort_by(|left, right| left.range.cmp(right.range));
        let mut canonical: Vec<PosixLockSegment> = Vec::with_capacity(segments.len());

        for segment in segments {
            if let Some(previous) = canonical.last_mut() {
                assert!(previous.owner.same_identity(&segment.owner));
                if previous.range.overlaps(segment.range) {
                    assert_eq!(
                        previous.mode, segment.mode,
                        "same POSIX lock owner has overlapping modes"
                    );
                }
                if previous.mode == segment.mode
                    && previous.range.touches_or_overlaps(segment.range)
                {
                    previous.range = previous.range.merge(segment.range);
                    continue;
                }
            }
            canonical.push(segment);
        }

        canonical
    }

    fn rebuild_assignment(
        segments: &[PosixLockSegment],
        owner: &PosixLockHolder,
        assigned: PosixLockRange,
        replacement: Option<(PosixLockMode, u32)>,
    ) -> Vec<PosixLockSegment> {
        let mut other_owners = Vec::new();
        let mut owner_segments = Vec::new();

        for segment in segments {
            if !segment.owner.same_identity(owner) {
                other_owners.push(segment.clone());
                continue;
            }
            if !segment.range.overlaps(assigned) {
                owner_segments.push(segment.clone());
                continue;
            }

            let (prefix, suffix) = segment.range.outside_parts(assigned);
            owner_segments.extend(prefix.into_iter().map(|range| PosixLockSegment {
                owner: owner.clone(),
                range,
                mode: segment.mode,
                report_tgid: segment.report_tgid,
            }));
            owner_segments.extend(suffix.into_iter().map(|range| PosixLockSegment {
                owner: owner.clone(),
                range,
                mode: segment.mode,
                report_tgid: segment.report_tgid,
            }));
        }

        if let Some((mode, report_tgid)) = replacement {
            owner_segments.push(PosixLockSegment {
                owner: owner.clone(),
                range: assigned,
                mode,
                report_tgid,
            });
        }

        other_owners.extend(Self::canonicalize_owner(owner_segments));
        other_owners.sort_by(|left, right| left.range.cmp(right.range));
        other_owners
    }

    fn query(
        &self,
        owner: &PosixLockHolder,
        range: PosixLockRange,
        mode: PosixLockMode,
    ) -> Option<PosixLockConflict> {
        Self::find_conflict(&self.segments.lock(), owner, range, mode)
    }

    fn set(
        &self,
        owner: &PosixLockHolder,
        range: PosixLockRange,
        mode: PosixLockMode,
        report_tgid: u32,
    ) -> Result<(), PosixLockConflict> {
        let replaced = {
            let mut segments = self.segments.lock();
            if let Some(conflict) = Self::find_conflict(&segments, owner, range, mode) {
                return Err(conflict);
            }
            let replacement =
                Self::rebuild_assignment(&segments, owner, range, Some((mode, report_tgid)));
            core::mem::replace(&mut *segments, replacement)
        };

        // A removed segment may own the last holder reference. Keep that drop
        // outside the domain guard so destruction cannot become lock re-entry.
        drop(replaced);
        Ok(())
    }

    fn unlock(&self, owner: &PosixLockHolder, range: PosixLockRange) {
        let replaced = {
            let mut segments = self.segments.lock();
            if !segments
                .iter()
                .any(|segment| segment.owner.same_identity(owner) && segment.range.overlaps(range))
            {
                return;
            }
            let replacement = Self::rebuild_assignment(&segments, owner, range, None);
            core::mem::replace(&mut *segments, replacement)
        };

        drop(replaced);
    }

    fn query_binding(
        &self,
        binding: &PosixLockBinding,
        range: PosixLockRange,
        mode: PosixLockMode,
    ) -> PosixLockQueryOutcome {
        let segments = self.segments.lock();
        if !binding.is_live() {
            return PosixLockQueryOutcome::BindingRetired;
        }
        match Self::find_conflict(&segments, binding.holder(), range, mode) {
            Some(conflict) => PosixLockQueryOutcome::Conflict(conflict),
            None => PosixLockQueryOutcome::Available,
        }
    }

    fn set_binding(
        &self,
        binding: &PosixLockBinding,
        range: PosixLockRange,
        mode: PosixLockMode,
        report_tgid: u32,
        wait_for_conflict: bool,
    ) -> PosixLockSetOutcome {
        loop {
            let replaced = {
                let mut segments = self.segments.lock();
                if !binding.is_live() {
                    return PosixLockSetOutcome::BindingRetired;
                }
                if let Some(conflict) =
                    Self::find_conflict(&segments, binding.holder(), range, mode)
                {
                    if !wait_for_conflict {
                        return PosixLockSetOutcome::Conflict(conflict);
                    }
                    None
                } else {
                    let replacement = Self::rebuild_assignment(
                        &segments,
                        binding.holder(),
                        range,
                        Some((mode, report_tgid)),
                    );
                    Some(core::mem::replace(&mut *segments, replacement))
                }
            };

            if let Some(replaced) = replaced {
                drop(replaced);
                // Same-owner replacement can remove ranges that blocked other
                // owners. Notification stays outside the domain guard and is
                // only a hint; every woken operation rechecks authoritative state.
                self.recheck.publish(usize::MAX, false);
                return PosixLockSetOutcome::Applied;
            }

            let predicate_ready = self.recheck.listen(false, || {
                let segments = self.segments.lock();
                !binding.is_live()
                    || Self::find_conflict(&segments, binding.holder(), range, mode).is_none()
            });
            if !predicate_ready {
                return PosixLockSetOutcome::Interrupted;
            }
        }
    }

    fn unlock_binding(
        &self,
        binding: &PosixLockBinding,
        range: PosixLockRange,
    ) -> PosixLockSetOutcome {
        let replaced = {
            let mut segments = self.segments.lock();
            if !binding.is_live() {
                return PosixLockSetOutcome::BindingRetired;
            }
            if !segments.iter().any(|segment| {
                segment.owner.same_identity(binding.holder()) && segment.range.overlaps(range)
            }) {
                None
            } else {
                let replacement =
                    Self::rebuild_assignment(&segments, binding.holder(), range, None);
                Some(core::mem::replace(&mut *segments, replacement))
            }
        };
        drop(replaced);
        // Unlock is an eligibility transition. Spurious publication for an
        // idempotent no-op is harmless because waiters always recheck.
        self.recheck.publish(usize::MAX, false);
        PosixLockSetOutcome::Applied
    }

    fn retire_binding(&self, binding: &PosixLockBinding) {
        let removed = {
            let mut segments = self.segments.lock();
            let old = core::mem::take(&mut *segments);
            let (removed, retained) = old
                .into_iter()
                .partition(|segment| segment.owner.same_identity(binding.holder()));
            *segments = retained;
            removed
        };

        // Holder references can be terminal. Keep their destruction outside
        // the inode-domain guard so cleanup cannot re-enter the lock owner.
        drop(removed);
        // Publish even when this holder had no grant: unpublishing the binding
        // itself can satisfy a blocked operation's liveness predicate.
        self.recheck.publish(usize::MAX, false);
    }
}

pub(crate) fn query_posix_lock(
    binding: &PosixLockBinding,
    range: PosixLockRange,
    mode: PosixLockMode,
) -> PosixLockQueryOutcome {
    binding
        .file()
        .inode()
        .posix_lock_domain()
        .query_binding(binding, range, mode)
}

pub(crate) fn set_posix_lock(
    binding: &PosixLockBinding,
    range: PosixLockRange,
    mode: PosixLockMode,
    report_tgid: u32,
    wait_for_conflict: bool,
) -> PosixLockSetOutcome {
    binding.file().inode().posix_lock_domain().set_binding(
        binding,
        range,
        mode,
        report_tgid,
        wait_for_conflict,
    )
}

pub(crate) fn unlock_posix_lock(
    binding: &PosixLockBinding,
    range: PosixLockRange,
) -> PosixLockSetOutcome {
    binding
        .file()
        .inode()
        .posix_lock_domain()
        .unlock_binding(binding, range)
}

pub(crate) fn retire_posix_locks(binding: &PosixLockBinding) {
    assert!(
        !binding.is_live(),
        "POSIX lock cleanup requires an unpublished fd-slot binding"
    );
    binding
        .file()
        .inode()
        .posix_lock_domain()
        .retire_binding(binding);
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::fs::{InodePerm, vfs_link, vfs_lookup, vfs_touch, vfs_unlink};

    fn finite(start: u64, end: u64) -> PosixLockRange {
        PosixLockRange::finite(start, end)
    }

    fn owner_snapshot(
        domain: &PosixLockDomain,
        owner: &PosixLockHolder,
    ) -> Vec<(PosixLockRange, PosixLockMode, u32)> {
        domain
            .segments
            .lock()
            .iter()
            .filter(|segment| segment.owner.same_identity(owner))
            .map(|segment| (segment.range, segment.mode, segment.report_tgid))
            .collect()
    }

    #[kunit]
    fn posix_range_assignment_replaces_splits_merges_and_unlocks() {
        let domain = PosixLockDomain::new();
        let owner = PosixLockHolder::new_for_kunit();

        domain
            .set(&owner, finite(0, 100), PosixLockMode::Read, 1)
            .unwrap();
        domain
            .set(&owner, finite(25, 75), PosixLockMode::Write, 2)
            .unwrap();
        assert_eq!(
            owner_snapshot(&domain, &owner),
            vec![
                (finite(0, 25), PosixLockMode::Read, 1),
                (finite(25, 75), PosixLockMode::Write, 2),
                (finite(75, 100), PosixLockMode::Read, 1),
            ]
        );

        domain
            .set(&owner, finite(20, 80), PosixLockMode::Read, 3)
            .unwrap();
        assert_eq!(owner_snapshot(&domain, &owner).len(), 1);
        assert_eq!(owner_snapshot(&domain, &owner)[0].0, finite(0, 100));

        domain
            .set(&owner, finite(100, 200), PosixLockMode::Write, 4)
            .unwrap();
        domain
            .set(&owner, finite(80, 120), PosixLockMode::Write, 5)
            .unwrap();
        domain.unlock(&owner, finite(20, 60));
        let after_unlock = owner_snapshot(&domain, &owner);
        assert_eq!(
            after_unlock
                .iter()
                .map(|(range, mode, _)| (*range, *mode))
                .collect::<Vec<_>>(),
            vec![
                (finite(0, 20), PosixLockMode::Read),
                (finite(60, 80), PosixLockMode::Read),
                (finite(80, 200), PosixLockMode::Write),
            ]
        );

        let before = after_unlock;
        domain.unlock(&owner, finite(30, 50));
        assert_eq!(owner_snapshot(&domain, &owner), before);
    }

    #[kunit]
    fn posix_conflict_rejects_without_partial_mutation() {
        let domain = PosixLockDomain::new();
        let first = PosixLockHolder::new_for_kunit();
        let second = PosixLockHolder::new_for_kunit();

        domain
            .set(&first, finite(0, 100), PosixLockMode::Read, 10)
            .unwrap();
        domain
            .set(&second, finite(20, 80), PosixLockMode::Read, 20)
            .unwrap();
        let before = owner_snapshot(&domain, &second);

        assert_eq!(
            domain
                .set(&second, finite(30, 60), PosixLockMode::Write, 21)
                .unwrap_err(),
            PosixLockConflict {
                range: finite(0, 100),
                mode: PosixLockMode::Read,
                report_tgid: 10,
            }
        );
        assert_eq!(owner_snapshot(&domain, &second), before);
    }

    #[kunit]
    fn posix_open_ended_query_reports_real_segment() {
        let domain = PosixLockDomain::new();
        let owner = PosixLockHolder::new_for_kunit();
        let other = PosixLockHolder::new_for_kunit();
        let open = PosixLockRange::open_ended(50);

        domain.set(&owner, open, PosixLockMode::Write, 30).unwrap();
        assert_eq!(
            domain.query(&other, finite(0, 50), PosixLockMode::Read),
            None
        );
        assert_eq!(
            domain.query(&other, finite(49, 51), PosixLockMode::Read),
            Some(PosixLockConflict {
                range: open,
                mode: PosixLockMode::Write,
                report_tgid: 30,
            })
        );
        assert!(
            domain
                .query(&other, PosixLockRange::open_ended(100), PosixLockMode::Read,)
                .is_some()
        );
    }

    #[kunit]
    fn posix_report_tgid_does_not_define_owner_or_coalescing() {
        let domain = PosixLockDomain::new();
        let owner = PosixLockHolder::new_for_kunit();
        let same_report_other_owner = PosixLockHolder::new_for_kunit();

        domain
            .set(&owner, finite(0, 10), PosixLockMode::Write, 40)
            .unwrap();
        domain
            .set(&owner, finite(10, 20), PosixLockMode::Write, 41)
            .unwrap();
        let canonical = owner_snapshot(&domain, &owner);
        assert_eq!(canonical.len(), 1);
        assert_eq!(canonical[0].0, finite(0, 20));
        assert_eq!(canonical[0].1, PosixLockMode::Write);
        assert!(matches!(canonical[0].2, 40 | 41));
        assert_eq!(
            domain.query(&owner, finite(0, 20), PosixLockMode::Read),
            None
        );
        assert!(
            domain
                .query(&same_report_other_owner, finite(0, 20), PosixLockMode::Read,)
                .is_some()
        );
    }

    #[kunit]
    fn posix_domain_follows_inode_identity_across_hard_links() {
        let source_path = Path::new("/kunit-posix-lock-source");
        let alias_path = Path::new("/kunit-posix-lock-alias");
        let other_path = Path::new("/kunit-posix-lock-other");
        let source = vfs_touch(source_path, InodePerm::all_rwx()).unwrap();
        vfs_link(source_path, alias_path).unwrap();
        let alias = vfs_lookup(alias_path).unwrap();
        let other_inode = vfs_touch(other_path, InodePerm::all_rwx()).unwrap();
        let owner = PosixLockHolder::new_for_kunit();
        let observer = PosixLockHolder::new_for_kunit();

        source
            .inode()
            .posix_lock_domain()
            .set(&owner, finite(0, 10), PosixLockMode::Write, 50)
            .unwrap();
        assert!(
            alias
                .inode()
                .posix_lock_domain()
                .query(&observer, finite(0, 10), PosixLockMode::Read)
                .is_some()
        );
        assert_eq!(
            other_inode.inode().posix_lock_domain().query(
                &observer,
                finite(0, 10),
                PosixLockMode::Read
            ),
            None
        );

        source
            .inode()
            .posix_lock_domain()
            .unlock(&owner, PosixLockRange::open_ended(0));
        vfs_unlink(alias_path).unwrap();
        vfs_unlink(source_path).unwrap();
        vfs_unlink(other_path).unwrap();
    }
}
