//! Memory mapping for user space.
//!
//! This is not a compatible layer for Linux's mmap, instead it's Anemone's
//! memory mapping primitives for user space, which, indeed, can be used to
//! implement Linux's mmap.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/mmap.2.html

use crate::prelude::{
    vma::{ForkPolicy, Protection, VmArea, VmFlags},
    vmo::{anon::AnonObject, shadow::ShadowObject},
    *,
};

/// Basic music notes for vm editing operations.
///
/// High-level tasks (e.g. map, unmap) are composed into a sequence(or to put it
/// better, a "sonata"?) of these operations, which are then executed together.
#[derive(Debug)]
enum VmOperation {
    /// Directly remove the whole [VmArea] starting at the given VPN.
    Remove { start: VirtPageNum },
    /// Trim the start of the [VmArea] starting at the given VPN by npages.
    ///
    /// Note the resulting [VmArea] is inserted back immediately, without use of
    /// [Self::Insert].
    TrimStart { start: VirtPageNum, npages: usize },
    /// Trim the end of the [VmArea] starting at the given VPN by npages.
    ///
    /// Note the resulting [VmArea] is inserted back immediately, without use of
    /// [Self::Insert].
    TrimEnd { start: VirtPageNum, npages: usize },
    /// Punch a hole in the [VmArea] starting at the given VPN, with the hole
    /// specified by the given range.
    ///
    /// Note the produced [VmArea]s are inserted back immediately, without use
    /// of [Self::Insert].
    ///
    /// This operation is guaranteed to produce exactly two resulting [VmArea]s,
    /// otherwise [Self::TrimStart] or [Self::TrimEnd] should be used.
    PunchHole {
        start: VirtPageNum,
        hole: VirtPageRange,
    },
    /// Insert a new [VmArea].
    Insert { vma: VmArea },
    /// As title.
    Unmap { range: VirtPageRange },
}

/// Named "Transaction" but it's not really a transaction in
/// traditional DB sense. It's more like a batch of operations to perform
/// together, and doesn't have any atomicity or rollback mechanism.
///
/// Just for convenience.
#[derive(Debug)]
pub(super) struct VmTransaction {
    ops: Vec<VmOperation>,
}

impl VmTransaction {
    /// Create an empty transaction.
    ///
    /// Note that it's always unrecommended to compose a transaction manually,
    /// as it's easy to make mistakes that violate the consistency.
    pub fn new() -> Self {
        Self { ops: Vec::new() }
    }

    pub fn remove(&mut self, start: VirtPageNum) -> &mut Self {
        self.ops.push(VmOperation::Remove { start });
        self
    }

    pub fn trim_start(&mut self, start: VirtPageNum, npages: usize) -> &mut Self {
        self.ops.push(VmOperation::TrimStart { start, npages });
        self
    }

    pub fn trim_end(&mut self, start: VirtPageNum, npages: usize) -> &mut Self {
        self.ops.push(VmOperation::TrimEnd { start, npages });
        self
    }

    pub fn punch_hole(&mut self, start: VirtPageNum, hole: VirtPageRange) -> &mut Self {
        self.ops.push(VmOperation::PunchHole { start, hole });
        self
    }

    pub fn insert(&mut self, vma: VmArea) -> &mut Self {
        self.ops.push(VmOperation::Insert { vma });
        self
    }

    pub fn unmap(&mut self, range: VirtPageRange) -> &mut Self {
        self.ops.push(VmOperation::Unmap { range });
        self
    }
}

#[derive(Debug)]
pub struct AnonymousMapping {
    /// (vpn, fixed)
    pub hint: Option<(VirtPageNum, bool)>,
    /// If hint is fixed, whether to clobber existing mappings. If true,
    /// existing mappings in the range will be unmapped.
    pub clobber: bool,
    pub npages: usize,
    pub prot: Protection,
    pub shared: bool,
    pub flags: VmFlags,
}

#[derive(Debug)]
pub struct FileMapping {
    pub hint: Option<(VirtPageNum, bool)>,
    pub clobber: bool,
    pub npages: usize,
    pub prot: Protection,
    pub shared: bool,
    pub flags: VmFlags,
    /// Offset, in page.
    pub poffset: usize,
    pub inode: InodeRef,
}

impl UserSpaceData {
    /// Map an anonymous memory region for the user space.
    ///
    /// Created [VmArea] will be backed by an [AnonObject].
    pub fn map_anonymous(&mut self, mapping: &AnonymousMapping) -> Result<VirtAddr, SysError> {
        let fixed = mapping.hint.is_some_and(|(_, fixed)| fixed);
        let vpn = match mapping.hint {
            Some((hint, true)) => hint,
            Some((hint, false)) => self
                .find_avail_range(mapping.npages, Some(hint))
                .ok_or(SysError::OutOfMemory)?,
            None => self
                .find_avail_range(mapping.npages, None)
                .ok_or(SysError::OutOfMemory)?,
        };
        let range = VirtPageRange::new(vpn, mapping.npages as u64);
        self.validate_range(range)?;

        let vmo = AnonObject::new(mapping.npages);
        let vma = VmArea::new(
            range,
            0,
            mapping.prot,
            if mapping.shared {
                ForkPolicy::Shared
            } else {
                ForkPolicy::CopyOnWrite
            },
            mapping.flags,
            Arc::new(vmo),
        );

        if fixed && mapping.clobber {
            self.replace_range(range, vma)?;
        } else {
            self.insert_vma(vma)?;
        }

        kdebugln!(
            "mapped anonymous range {range:?} with prot={:?}, shared={}, flags={:?}",
            mapping.prot,
            mapping.shared,
            mapping.flags
        );

        Ok(vpn.to_virt_addr())
    }

    pub fn map_file(&mut self, mapping: &FileMapping) -> Result<VirtAddr, SysError> {
        let fixed = mapping.hint.is_some_and(|(_, fixed)| fixed);
        let vpn = match mapping.hint {
            Some((hint, true)) => hint,
            Some((hint, false)) => self
                .find_avail_range(mapping.npages, Some(hint))
                .ok_or(SysError::OutOfMemory)?,
            None => self
                .find_avail_range(mapping.npages, None)
                .ok_or(SysError::OutOfMemory)?,
        };
        let range = VirtPageRange::new(vpn, mapping.npages as u64);
        self.validate_range(range)?;

        if let Some(m) = mapping.inode.mapping() {
            let vma = VmArea::new(
                range,
                mapping.poffset,
                mapping.prot,
                if mapping.shared {
                    ForkPolicy::Shared
                } else {
                    ForkPolicy::CopyOnWrite
                },
                mapping.flags,
                if mapping.shared {
                    m.clone()
                } else {
                    Arc::new(ShadowObject::new(m.clone()))
                },
            );

            if fixed && mapping.clobber {
                self.replace_range(range, vma)?;
            } else {
                self.insert_vma(vma)?;
            }

            kdebugln!(
                "mapped file-backed range {range:?} with prot={:?}, shared={}, flags={:?}, poffset={:#x} for inode {}",
                mapping.prot,
                mapping.shared,
                mapping.flags,
                mapping.poffset,
                mapping.inode.ino().get()
            );

            Ok(vpn.to_virt_addr())
        } else {
            Err(SysError::NotSupported)
        }
    }

    /// Try to insert the given [VmArea] into user space.
    ///
    /// Umm... Yep, in most cases you probably don't want to call this method
    /// directly, however there are always some corner cases that cannot be
    /// handled by higher-level APIs, e.g. `replace_range`, and you just want to
    /// insert a new VMA (probably highly customized) without any fancy logic,
    /// then this method is here for you.
    pub fn insert_vma(&mut self, vma: VmArea) -> Result<(), SysError> {
        if !self.is_range_avail(*vma.range()) {
            return Err(SysError::AlreadyMapped);
        }

        assert!(self.vmas.insert(vma.range().start(), vma).is_none());
        Ok(())
    }

    /// Unmap the given virtual page range.
    ///
    /// TODO: explain the semantics of unmapping, especially when the range
    /// partially intersects with existing mappings.
    pub fn unmap(&mut self, range: VirtPageRange) -> Result<(), SysError> {
        let tx = self.compose_unmap_range(range)?;
        unsafe {
            self.run_transaction(tx);
        }

        kdebugln!("unmapped region {:#x?}", range);

        Ok(())
    }

    /// Change protection on a fully-mapped range.
    ///
    /// If any page in the target range falls into a hole, the whole operation
    /// fails without modifying any VMA or PTE state.
    pub fn protect_range(
        &mut self,
        range: VirtPageRange,
        prot: Protection,
    ) -> Result<(), SysError> {
        let tx = self.compose_protect_range(range, prot)?;
        unsafe {
            self.run_transaction(tx);
        }
        kdebugln!("changed protection on range {:#x?} to {:?}", range, prot);
        Ok(())
    }
}

// these methods seems inappropriate to be public, but other modules may need
// them?
impl UserSpaceData {
    /// Replace the given virtual page range with the new [VmArea].
    ///
    /// If there are existing mappings in the given range, they will be unmapped
    /// first, with corresponding [VmArea]s removed or tailored.
    pub(super) fn replace_range(
        &mut self,
        range: VirtPageRange,
        new: VmArea,
    ) -> Result<(), SysError> {
        if *new.range() != range {
            return Err(SysError::InvalidArgument);
        }
        if new.reservation().is_some() {
            panic!("replace_range must not install system-managed reservations");
        }

        let mut tx = self.compose_unmap_range(range)?;
        tx.ops.push(VmOperation::Insert { vma: new });
        unsafe {
            self.run_transaction(tx);
        }

        Ok(())
    }
}

// here lies helper methods for vm editing.
impl UserSpaceData {
    /// Collect start VPNs of those [VmArea]s whose ranges intersect with the
    /// given range.
    ///
    /// The returned VPNs are is ascending-sorted.
    fn find_intersection_starts(&self, range: VirtPageRange) -> Vec<VirtPageNum> {
        self.vmas
            .range(..range.end())
            .filter_map(|(start, vma)| vma.range().intersects(&range).then_some(*start))
            .collect()
    }

    /// Collect all VMA starts intersecting `range`, while requiring the whole
    /// range to be continuously covered by existing mappings.
    ///
    /// If there's any gap in the coverage, an error is returned.
    fn find_intersection_starts_covering(
        &self,
        range: VirtPageRange,
    ) -> Result<Vec<VirtPageNum>, SysError> {
        self.validate_range(range)?;

        let starts = self.find_intersection_starts(range);
        let mut covered_until = range.start();

        for start in starts.iter().copied() {
            let vma = self
                .vmas
                .get(&start)
                .expect("intersection key must resolve to a VMA");

            if vma.range().start() > covered_until {
                return Err(SysError::RangeNotMapped);
            }

            let intersect_end = if vma.range().end() < range.end() {
                vma.range().end()
            } else {
                range.end()
            };
            if intersect_end > covered_until {
                covered_until = intersect_end;
            }
            if covered_until >= range.end() {
                return Ok(starts);
            }
        }

        Err(SysError::RangeNotMapped)
    }

    /// Find an available virtual page range of `npages` pages, with an optional
    /// hint.
    fn find_avail_range(&self, npages: usize, hint: Option<VirtPageNum>) -> Option<VirtPageNum> {
        if npages == 0 {
            panic!(
                "mapping zero pages is not allowed. this might indicate a potential bug in caller code."
            );
        }

        let npages = npages as u64;

        if let Some(hint) = hint {
            let hint_range = VirtPageRange::new(hint, npages);
            if self.is_range_avail(hint_range) {
                return Some(hint);
            }
        }

        let mut gap_end = KernelLayout::USPACE_TOP_VPN;

        for vma in self.vmas.values().rev() {
            let gap_start = vma.range().end();
            if gap_end.get() >= gap_start.get() + npages {
                return Some(gap_end - npages);
            }
            gap_end = vma.range().start();
        }

        return None;
    }

    fn validate_range(&self, range: VirtPageRange) -> Result<(), SysError> {
        if range.npages() == 0
            || range.end() > KernelLayout::USPACE_TOP_VPN
            || range.start().get() == 0
        {
            return Err(SysError::InvalidArgument);
        }
        Ok(())
    }

    /// Zero-length ranges are considered invalid.
    fn is_range_avail(&self, range: VirtPageRange) -> bool {
        if self.validate_range(range).is_err() {
            return false;
        }

        !self.vmas.values().any(|vma| vma.range().intersects(&range))
    }
}

// Our vm editing enging.
//
// These operations are executed while holding the lock on [UserSpaceData], so
// the consistency is guaranteed.
//
// Basically, if a operation results in a new VMA, it should be inserted back
// immediately. No additional `Insert` operation. However, `Unmap` operations
// are deferred until the end, to make sure all the VMA edits are done before
// any unmapping happens.
impl UserSpaceData {
    /// Compose a transaction to unmap the given range.
    ///
    /// The composed transaction is guaranteed to be valid until [UserSpaceData]
    /// is unlocked.
    fn compose_unmap_range(&self, range: VirtPageRange) -> Result<VmTransaction, SysError> {
        self.validate_range(range)?;

        let mut tx = VmTransaction::new();

        for start in self.find_intersection_starts(range) {
            let vma = self
                .vmas
                .get(&start)
                .expect("intersection key must resolve to a VMA");

            if vma.reservation().is_some() {
                // stack or heap cannot be unmapped.
                return Err(SysError::PermissionDenied);
            }

            let cut_start = if vma.range().start() > range.start() {
                vma.range().start()
            } else {
                range.start()
            };
            let cut_end = if vma.range().end() < range.end() {
                vma.range().end()
            } else {
                range.end()
            };
            let cut_range = VirtPageRange::new(cut_start, cut_end - cut_start);

            if range.covers(vma.range()) {
                tx.remove(start);
            } else if range.start() <= vma.range().start() {
                tx.trim_start(start, (cut_end - vma.range().start()) as usize);
            } else if vma.range().end() <= range.end() {
                tx.trim_end(start, (vma.range().end() - cut_start) as usize);
            } else {
                tx.punch_hole(start, cut_range);
            }

            tx.unmap(cut_range);
        }

        Ok(tx)
    }

    fn compose_protect_range(
        &self,
        range: VirtPageRange,
        prot: Protection,
    ) -> Result<VmTransaction, SysError> {
        let mut tx = VmTransaction::new();

        for start in self.find_intersection_starts_covering(range)? {
            let vma = self
                .vmas
                .get(&start)
                .expect("intersection key must resolve to a VMA");

            if vma.reservation().is_some() {
                return Err(SysError::PermissionDenied);
            }
            if vma.prot() == prot {
                continue;
            }

            let cut_start = if vma.range().start() > range.start() {
                vma.range().start()
            } else {
                range.start()
            };
            let cut_end = if vma.range().end() < range.end() {
                vma.range().end()
            } else {
                range.end()
            };
            let cut_range = VirtPageRange::new(cut_start, cut_end - cut_start);

            let (left, maybe_target) = vma
                .clone()
                .split(cut_start)
                .expect("protect composer must split on an intersecting VMA boundary");
            let (target, right) = maybe_target
                .expect("protect composer must retain the intersecting target segment")
                .split(cut_end)
                .expect("protect composer must split the target segment end");
            let mut target = target.expect("protect composer must produce a target VMA");
            target.set_prot(prot);

            tx.remove(start);
            if let Some(left) = left {
                tx.insert(left);
            }
            tx.insert(target);
            if let Some(right) = right {
                tx.insert(right);
            }
            tx.unmap(cut_range);
        }

        Ok(tx)
    }

    /// Apply the given transaction.
    ///
    /// This operation should never return an error, as the transaction should
    /// be guaranteed to be valid by the composer.
    ///
    /// In almost all cases, you should not call this method directly, but use
    /// higher-level APIs like `replace_range` instead, which guarantees the
    /// validity of the composed transaction.
    pub(super) unsafe fn run_transaction(&mut self, tx: VmTransaction) {
        let mut unmaps = Vec::new();

        for op in tx.ops {
            match op {
                VmOperation::Remove { start } => {
                    let removed = self
                        .vmas
                        .remove(&start)
                        .expect("remove op must target an existing VMA");
                    assert!(removed.reservation().is_none());
                },
                VmOperation::TrimStart { start, npages } => {
                    let mut vma = self
                        .vmas
                        .remove(&start)
                        .expect("trim-start op must target an existing VMA");
                    assert!(vma.reservation().is_none());
                    vma.trim_start(npages)
                        .expect("trim-start op must keep a non-empty VMA");
                    assert!(self.vmas.insert(vma.range().start(), vma).is_none());
                },
                VmOperation::TrimEnd { start, npages } => {
                    let vma = self
                        .vmas
                        .get_mut(&start)
                        .expect("trim-end op must target an existing VMA");
                    assert!(vma.reservation().is_none());
                    vma.trim_end(npages)
                        .expect("trim-end op must keep a non-empty VMA");
                },
                VmOperation::PunchHole { start, hole } => {
                    let vma = self
                        .vmas
                        .remove(&start)
                        .expect("punch-hole op must target an existing VMA");
                    assert!(vma.reservation().is_none());

                    let (left, right) = {
                        let (left, middle_and_right) = vma
                            .split(hole.start())
                            .expect("hole start must split inside the original VMA");
                        let middle_and_right =
                            middle_and_right.expect("hole split must leave a middle/right portion");
                        let (_, right) = middle_and_right
                            .split(hole.end())
                            .expect("hole end must split inside the remaining VMA");

                        (
                            left.expect("punch-hole op must produce a left VMA"),
                            right.expect("punch-hole op must produce a right VMA"),
                        )
                    };

                    assert!(self.vmas.insert(left.range().start(), left).is_none());
                    assert!(self.vmas.insert(right.range().start(), right).is_none());
                },
                VmOperation::Insert { vma } => {
                    assert!(vma.reservation().is_none());
                    assert!(self.is_range_avail(*vma.range()));
                    assert!(self.vmas.insert(vma.range().start(), vma).is_none());
                },
                VmOperation::Unmap { range } => unmaps.push(range),
            }
        }

        // unmapping must be performed after all vm area edits.
        if !unmaps.is_empty() {
            let mut mapper = self.table.mapper();

            for range in unmaps {
                unsafe {
                    mapper.try_unmap(Unmapping { range });
                }
            }

            PagingArch::tlb_shootdown_all();
        }
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn fixed_mapping(
        svpn: VirtPageNum,
        npages: usize,
        prot: Protection,
        shared: bool,
        clobber: bool,
    ) -> AnonymousMapping {
        AnonymousMapping {
            hint: Some((svpn, true)),
            clobber,
            npages,
            prot,
            shared,
            flags: VmFlags::empty(),
        }
    }

    fn anon_vmas(uspace: &UserSpaceData) -> Vec<(VirtPageRange, Protection)> {
        uspace
            .vmas
            .values()
            .filter(|vma| vma.reservation().is_none())
            .map(|vma| (*vma.range(), vma.prot()))
            .collect()
    }

    #[kunit]
    fn unmap_middle_of_anonymous_vma_punches_hole() {
        let mut uspace = UserSpaceData::new().expect("user space setup should succeed");
        let base = uspace.stack_vma().range().start() - 32;

        uspace
            .map_anonymous(&fixed_mapping(
                base,
                8,
                Protection::READ | Protection::WRITE,
                false,
                false,
            ))
            .expect("anonymous mapping should succeed");

        uspace
            .unmap(VirtPageRange::new(base + 3, 2))
            .expect("middle unmap should succeed");

        assert_eq!(
            anon_vmas(&uspace),
            vec![
                (
                    VirtPageRange::new(base, 3),
                    Protection::READ | Protection::WRITE
                ),
                (
                    VirtPageRange::new(base + 5, 3),
                    Protection::READ | Protection::WRITE,
                ),
            ]
        );
    }

    #[kunit]
    fn protect_range_splits_vma_and_invalidates_present_ptes() {
        let mut uspace = UserSpaceData::new().expect("user space setup should succeed");
        let base = uspace.stack_vma().range().start() - 48;

        uspace
            .map_anonymous(&fixed_mapping(
                base,
                8,
                Protection::READ | Protection::WRITE,
                false,
                false,
            ))
            .expect("anonymous mapping should succeed");
        uspace
            .inject_page_fault((base + 3).to_virt_addr(), PageFaultType::Read)
            .expect("faulting mapped page should succeed");
        assert!(
            uspace
                .page_table_mut()
                .mapper()
                .translate(base + 3)
                .is_some()
        );

        uspace
            .protect_range(VirtPageRange::new(base + 2, 2), Protection::READ)
            .expect("protecting a mapped range should succeed");

        assert_eq!(
            anon_vmas(&uspace),
            vec![
                (
                    VirtPageRange::new(base, 2),
                    Protection::READ | Protection::WRITE
                ),
                (VirtPageRange::new(base + 2, 2), Protection::READ),
                (
                    VirtPageRange::new(base + 4, 4),
                    Protection::READ | Protection::WRITE,
                ),
            ]
        );
        assert!(
            uspace
                .page_table_mut()
                .mapper()
                .translate(base + 3)
                .is_none()
        );
    }

    #[kunit]
    fn protect_range_rejects_holes_and_reservations() {
        let mut uspace = UserSpaceData::new().expect("user space setup should succeed");
        let base = uspace.stack_vma().range().start() - 64;

        uspace
            .map_anonymous(&fixed_mapping(
                base,
                2,
                Protection::READ | Protection::WRITE,
                false,
                false,
            ))
            .expect("anonymous mapping should succeed");

        assert_eq!(
            uspace.protect_range(VirtPageRange::new(base + 1, 4), Protection::READ),
            Err(SysError::RangeNotMapped)
        );
        // assert_eq!(
        //     uspace.protect_range(VirtPageRange::new(heap_start, 1),
        // Protection::READ),     Err(SysError::PermissionDenied)
        // );
        // assert_eq!(
        //     uspace.unmap(VirtPageRange::new(heap_start, 1)),
        //     Err(SysError::PermissionDenied)
        // );
    }

    #[kunit]
    fn fixed_replace_and_noreplace_follow_unified_editing() {
        let mut uspace = UserSpaceData::new().expect("user space setup should succeed");
        let base = uspace.stack_vma().range().start() - 80;

        uspace
            .map_anonymous(&fixed_mapping(
                base,
                8,
                Protection::READ | Protection::WRITE,
                false,
                false,
            ))
            .expect("initial anonymous mapping should succeed");

        assert_eq!(
            uspace.map_anonymous(&fixed_mapping(base + 2, 4, Protection::READ, false, false)),
            Err(SysError::AlreadyMapped)
        );

        uspace
            .map_anonymous(&fixed_mapping(base + 2, 4, Protection::READ, false, true))
            .expect("fixed replace should reuse the unified editing path");

        assert_eq!(
            anon_vmas(&uspace),
            vec![
                (
                    VirtPageRange::new(base, 2),
                    Protection::READ | Protection::WRITE
                ),
                (VirtPageRange::new(base + 2, 4), Protection::READ),
                (
                    VirtPageRange::new(base + 6, 2),
                    Protection::READ | Protection::WRITE,
                ),
            ]
        );
    }

    #[kunit]
    fn anonymous_fork_keeps_shared_backing_and_cow_private() {
        let mut parent = UserSpaceData::new().expect("user space setup should succeed");
        let shared_base = parent.stack_vma().range().start() - 96;
        let private_base = parent.stack_vma().range().start() - 112;

        parent
            .map_anonymous(&fixed_mapping(
                shared_base,
                2,
                Protection::READ | Protection::WRITE,
                true,
                false,
            ))
            .expect("shared anonymous mapping should succeed");
        parent
            .map_anonymous(&fixed_mapping(
                private_base,
                2,
                Protection::READ | Protection::WRITE,
                false,
                false,
            ))
            .expect("private anonymous mapping should succeed");

        let child = parent.fork().expect("fork should succeed");

        let parent_shared = parent
            .find_vma(shared_base.to_virt_addr())
            .expect("shared parent VMA must exist");
        let child_shared = child
            .find_vma(shared_base.to_virt_addr())
            .expect("shared child VMA must exist");
        assert!(Arc::ptr_eq(parent_shared.backing(), child_shared.backing()));

        let parent_private = parent
            .find_vma(private_base.to_virt_addr())
            .expect("private parent VMA must exist");
        let child_private = child
            .find_vma(private_base.to_virt_addr())
            .expect("private child VMA must exist");
        assert!(!Arc::ptr_eq(
            parent_private.backing(),
            child_private.backing(),
        ));
    }
}
