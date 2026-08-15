//! Virtual memory area.
//!
//! Reference:
//! - https://fuchsia.dev/fuchsia-src/reference/kernel_objects/vm_address_region

use crate::{
    mm::paging::LeafPteCommit,
    prelude::{
        vmo::{ResolvedFrame, VmObject, shadow::ShadowObject},
        *,
    },
};

/// Determines how a [VmArea] is [VmArea::fork]ed.
#[derive(Debug, Clone, Copy)]
pub enum ForkPolicy {
    /// Child process shares the same backing [VmObject] with parent. Changes in
    /// one process will affect the other.
    Shared,
    /// Both parent and child process get a [ShadowObject] pointing to the
    /// original backing. Writing will immediately trigger copy-on-write, so
    /// changes in one process won't affect the other.
    CopyOnWrite,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Protection: usize {
        const READ = 1 << 0;
        const WRITE = 1 << 1;
        const EXECUTE = 1 << 2;
    }
}

impl From<PageFaultType> for Protection {
    fn from(value: PageFaultType) -> Self {
        match value {
            PageFaultType::Read => Self::READ,
            PageFaultType::Write => Self::WRITE,
            PageFaultType::Execute => Self::EXECUTE,
        }
    }
}

impl From<Protection> for PteFlags {
    fn from(value: Protection) -> Self {
        let mut flags = PteFlags::USER;
        if value.contains(Protection::READ) {
            flags |= PteFlags::READ;
        }
        if value.contains(Protection::WRITE) {
            flags |= PteFlags::WRITE;
        }
        if value.contains(Protection::EXECUTE) {
            flags |= PteFlags::EXECUTE;
        }
        flags
    }
}

impl From<PteFlags> for Protection {
    fn from(value: PteFlags) -> Self {
        let mut prot = Protection::empty();
        if value.contains(PteFlags::READ) {
            prot |= Protection::READ;
        }
        if value.contains(PteFlags::WRITE) {
            prot |= Protection::WRITE;
        }
        if value.contains(PteFlags::EXECUTE) {
            prot |= Protection::EXECUTE;
        }
        prot
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct VmFlags: usize {
        /// For ordinary grow-down VMAs managed by generic VMA policy.
        /// Stack reservation growth is handled separately by [UserSpace].
        ///
        /// Currently not supported.
        const GROW_DOWN = 1 << 0;
    }
}

/// System-managed reservation type. This is orthogonal to the actual mapping
/// type, and is used to mark some special VMAs that require special handling in
/// some scenarios.
///
/// **Invariant: A [UserSpace] has only 1 stack and 1 heap reservation.**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmReservation {
    Stack,
    Heap,
    Guard,
}

/// Virtual memory area, within a [UserSpace].
///
/// [VmArea] is tied to a specific [UserSpace], but current design does not
/// force that. For example, you could pass a [Mapper] from another [UserSpace]
/// to [VmArea::resolve_page_access]. We should refactor some APIs to forbid
/// that invalid use later.
#[derive(Debug, Clone)]
pub struct VmArea {
    /// `range` along with `poffset` determines where this VMA views into the
    /// underlying [VmObject].
    range: VirtPageRange,
    /// Starting frame index in underlying [VmObject] that corresponds to
    /// `range.start()`.
    poffset: usize,
    /// Protection of this VMA.
    ///
    /// This maybe more than the actual protection of the mapped page.
    prot: Protection,
    /// Fork policy of this VMA. Mostly used when cloning a task without
    /// `CLONE_VM`.
    on_fork: ForkPolicy,
    /// Auxiliary flags of this VMA.
    flags: VmFlags,
    /// System-managed reservation type.
    reservation: Option<VmReservation>,
    /// The underlying virtual memory object.
    backing: Arc<dyn VmObject>,
}

impl VmArea {
    pub fn new(
        range: VirtPageRange,
        poffset: usize,
        prot: Protection,
        on_fork: ForkPolicy,
        flags: VmFlags,
        backing: Arc<dyn VmObject>,
    ) -> Self {
        Self::new_internal(range, poffset, prot, on_fork, flags, None, backing)
    }

    pub(super) fn new_reserved(
        range: VirtPageRange,
        poffset: usize,
        prot: Protection,
        on_fork: ForkPolicy,
        flags: VmFlags,
        reservation: VmReservation,
        backing: Arc<dyn VmObject>,
    ) -> Self {
        Self::new_internal(
            range,
            poffset,
            prot,
            on_fork,
            flags,
            Some(reservation),
            backing,
        )
    }

    fn new_internal(
        range: VirtPageRange,
        poffset: usize,
        prot: Protection,
        on_fork: ForkPolicy,
        flags: VmFlags,
        reservation: Option<VmReservation>,
        backing: Arc<dyn VmObject>,
    ) -> Self {
        Self {
            range,
            poffset,
            prot,
            on_fork,
            flags,
            reservation,
            backing,
        }
    }

    /// Get the range of this VMA.
    pub fn range(&self) -> &VirtPageRange {
        &self.range
    }

    /// As title.
    pub fn set_range(&mut self, new: VirtPageRange) {
        self.range = new;
    }

    /// As title.
    pub fn set_backing(&mut self, new: Arc<dyn VmObject>) {
        self.backing = new;
    }

    /// Get the protection of this VMA.
    pub fn prot(&self) -> Protection {
        self.prot
    }
    /// Set the protection of this VMA.
    pub fn set_prot(&mut self, new: Protection) {
        self.prot = new;
    }

    /// Get the fork policy of this VMA.
    pub fn on_fork(&self) -> ForkPolicy {
        self.on_fork
    }

    /// Set the fork policy of this VMA.
    pub fn switch_fork_policy(&mut self, new: ForkPolicy) {
        self.on_fork = new;
    }

    /// Get the auxiliary flags of this VMA.
    pub fn flags(&self) -> VmFlags {
        self.flags
    }

    /// Get the reservation type of this VMA, if any.
    pub fn reservation(&self) -> Option<VmReservation> {
        self.reservation
    }

    /// Get the underlying virtual memory object of this VMA.
    pub fn backing(&self) -> &Arc<dyn VmObject> {
        &self.backing
    }

    /// Translate a virtual page inside this VMA to an object-relative page
    /// index.
    pub fn vmo_pidx(&self, vpn: VirtPageNum) -> usize {
        debug_assert!(self.range.contains(vpn));
        self.poffset + (vpn - self.range.start()) as usize
    }

    fn resolved_flags(&self, resolved: &ResolvedFrame) -> PteFlags {
        let mut flags: PteFlags = PteFlags::from(self.prot) | PteFlags::USER;
        if !resolved.writable {
            flags -= PteFlags::WRITE;
        }
        flags
    }

    fn map_page(
        &mut self,
        mapper: &mut Mapper,
        vpn: VirtPageNum,
        access: PageFaultType,
    ) -> Result<LeafPteCommit, SysError> {
        debug_assert!(self.range.contains(vpn));

        if !self.prot.contains(access.into()) {
            return Err(SysError::PermissionDenied);
        }

        let pidx = self.vmo_pidx(vpn);
        let resolved = self.backing.resolve_frame(pidx, access)?;
        let flags = self.resolved_flags(&resolved);

        unsafe { mapper.commit_leaf(vpn, resolved.frame.ppn(), flags) }
    }

    /// Best-effort map absent followers after an exact user fault succeeded.
    ///
    /// The caller holds the owning [`UserSpace`] mutex, so the admission
    /// translation remains stable until each leaf commit. Existing mappings
    /// terminate the contiguous locality window and are never overwritten.
    pub(super) fn resolve_page_access_ahead(
        &mut self,
        mapper: &mut Mapper,
        demand_vpn: VirtPageNum,
        end: VirtPageNum,
        access: PageFaultType,
    ) {
        assert!(self.range.contains(demand_vpn));
        assert!(demand_vpn < end && end <= self.range.end());

        let request_end = self.poffset + (end - self.range.start()) as usize;
        let mut vpn = demand_vpn + 1;
        while vpn < end {
            if mapper.translate(vpn).is_some() {
                break;
            }

            let pidx = self.vmo_pidx(vpn);
            let resolved = match self.backing.resolve_frame_ahead(pidx, request_end, access) {
                Ok(Some(resolved)) => resolved,
                Ok(None) | Err(_) => break,
            };
            let flags = self.resolved_flags(&resolved);
            match unsafe { mapper.commit_leaf(vpn, resolved.frame.ppn(), flags) } {
                Ok(commit) => assert_eq!(
                    commit,
                    LeafPteCommit::Added,
                    "fault-ahead may only install a previously absent leaf"
                ),
                Err(SysError::OutOfMemory) => break,
                Err(err) => panic!("fault-ahead leaf commit failed unexpectedly: {err:?}"),
            }
            vpn += 1;
        }
    }

    /// Resolve one page access in this VMA.
    ///
    /// `addr` is guaranteed to be in the range of this VMA.
    ///
    /// The caller owns local completion for the returned commit relation.
    pub(super) fn resolve_page_access(
        &mut self,
        mapper: &mut Mapper,
        addr: VirtAddr,
        access: PageFaultType,
    ) -> Result<LeafPteCommit, SysError> {
        let vpn = addr.page_down();
        debug_assert!(self.range.contains(vpn));

        self.map_page(mapper, vpn, access)
    }

    /// Fork this VMA and report whether the parent PTEs were restricted.
    ///
    /// The address-space owner performs the single local flush and remote
    /// completion after all VMAs have been processed.
    pub(super) fn fork(&mut self, mapper: &mut Mapper) -> (Self, bool) {
        match self.on_fork {
            ForkPolicy::Shared => (self.clone(), false),
            ForkPolicy::CopyOnWrite => {
                let restricted = self.prot.contains(Protection::WRITE);
                if restricted {
                    unsafe {
                        mapper.change_flags(
                            self.range,
                            |_, flags| {
                                if flags.contains(PteFlags::WRITE) {
                                    Some(flags - PteFlags::WRITE)
                                } else {
                                    None
                                }
                            },
                            TraverseOrder::PreOrder,
                        );
                    }
                }

                let original = self.backing.clone();
                self.backing = Arc::new(ShadowObject::new(original.clone()));
                let child: Arc<dyn VmObject> = Arc::new(ShadowObject::new(original));
                (
                    Self {
                        range: self.range,
                        poffset: self.poffset,
                        prot: self.prot,
                        on_fork: self.on_fork,
                        flags: self.flags,
                        reservation: self.reservation,
                        backing: child,
                    },
                    restricted,
                )
            },
        }
    }
}

impl VmArea {
    /// Most primitive and most powerful way to tailor a VMA.
    pub(super) fn split(self, at: VirtPageNum) -> Result<(Option<Self>, Option<Self>), SysError> {
        if at < self.range.start() || at > self.range.end() {
            return Err(SysError::InvalidArgument);
        }

        let left = if at > self.range.start() {
            Some(Self {
                range: VirtPageRange::new(self.range.start(), at - self.range.start()),
                poffset: self.poffset,
                prot: self.prot,
                on_fork: self.on_fork,
                flags: self.flags,
                reservation: self.reservation,
                backing: self.backing.clone(),
            })
        } else {
            None
        };

        let right = if at < self.range.end() {
            Some(Self {
                range: VirtPageRange::new(at, self.range.end() - at),
                poffset: self.poffset + (at - self.range.start()) as usize,
                prot: self.prot,
                on_fork: self.on_fork,
                flags: self.flags,
                reservation: self.reservation,
                backing: self.backing.clone(),
            })
        } else {
            None
        };

        Ok((left, right))
    }

    // coalesce is not supported for now.

    /// Trim the first `npages` pages of this VMA.
    ///
    /// Trying to trim the whole region is considered invalid.
    pub(super) fn trim_start(&mut self, npages: usize) -> Result<(), SysError> {
        if npages as u64 >= self.range.npages() {
            return Err(SysError::InvalidArgument);
        }

        self.range = VirtPageRange::new(
            self.range.start() + npages as u64,
            self.range.npages() - npages as u64,
        );
        self.poffset += npages;

        Ok(())
    }

    /// Trim the last `npages` pages of this VMA.
    ///
    /// Trying to trim the whole region is considered invalid.
    pub(super) fn trim_end(&mut self, npages: usize) -> Result<(), SysError> {
        if npages as u64 >= self.range.npages() {
            return Err(SysError::InvalidArgument);
        }

        self.range = VirtPageRange::new(self.range.start(), self.range.npages() - npages as u64);

        Ok(())
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    struct AheadObject {
        frames: Box<[FrameHandle]>,
        ahead: RwLock<Vec<(usize, usize, PageFaultType)>>,
        stop_at: Option<usize>,
    }

    impl AheadObject {
        fn new(npages: usize, stop_at: Option<usize>) -> Self {
            let frames = (0..npages)
                .map(|_| unsafe {
                    alloc_frame_zeroed()
                        .expect("fault-ahead KUnit frame allocation should succeed")
                        .into_frame_handle()
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            Self {
                frames,
                ahead: RwLock::new(Vec::new()),
                stop_at,
            }
        }

        fn resolved(&self, pidx: usize) -> Result<ResolvedFrame, SysError> {
            Ok(ResolvedFrame {
                frame: self.frames.get(pidx).ok_or(SysError::NotMapped)?.clone(),
                writable: true,
            })
        }
    }

    impl VmObject for AheadObject {
        fn resolve_frame(
            &self,
            pidx: usize,
            _access: PageFaultType,
        ) -> Result<ResolvedFrame, SysError> {
            self.resolved(pidx)
        }

        fn resolve_frame_ahead(
            &self,
            pidx: usize,
            request_end: usize,
            access: PageFaultType,
        ) -> Result<Option<ResolvedFrame>, SysError> {
            self.ahead.write().push((pidx, request_end, access));
            if self.stop_at == Some(pidx) {
                return Ok(None);
            }
            self.resolved(pidx).map(Some)
        }
    }

    fn test_vma(backing: Arc<dyn VmObject>) -> (VmArea, PageTable, VirtPageNum) {
        let base = VirtPageNum::new(0x40000);
        (
            VmArea::new(
                VirtPageRange::new(base, 6),
                3,
                Protection::READ | Protection::WRITE,
                ForkPolicy::Shared,
                VmFlags::empty(),
                backing,
            ),
            PageTable::new().expect("fault-ahead KUnit page table allocation should succeed"),
            base,
        )
    }

    #[kunit]
    fn ahead_maps_only_absent_added_prefix_with_one_fixed_request_end() {
        let object = Arc::new(AheadObject::new(9, Some(7)));
        let (mut vma, mut table, base) = test_vma(object.clone());
        let mut mapper = table.mapper();

        assert_eq!(
            vma.resolve_page_access(&mut mapper, base.to_virt_addr(), PageFaultType::Read)
                .unwrap(),
            LeafPteCommit::Added
        );
        vma.resolve_page_access_ahead(&mut mapper, base, base + 5, PageFaultType::Read);

        for offset in 0..4 {
            assert!(mapper.translate(base + offset).is_some());
        }
        assert!(mapper.translate(base + 4).is_none());
        assert_eq!(
            *object.ahead.read(),
            vec![
                (4, 8, PageFaultType::Read),
                (5, 8, PageFaultType::Read),
                (6, 8, PageFaultType::Read),
                (7, 8, PageFaultType::Read),
            ]
        );
    }

    #[kunit]
    fn ahead_stops_before_an_existing_leaf_without_replacing_it() {
        let object = Arc::new(AheadObject::new(9, None));
        let (mut vma, mut table, base) = test_vma(object.clone());
        let mut mapper = table.mapper();

        vma.resolve_page_access(&mut mapper, base.to_virt_addr(), PageFaultType::Write)
            .unwrap();
        vma.resolve_page_access(&mut mapper, (base + 2).to_virt_addr(), PageFaultType::Write)
            .unwrap();
        let existing = mapper.translate(base + 2).unwrap();

        vma.resolve_page_access_ahead(&mut mapper, base, base + 5, PageFaultType::Write);

        assert!(mapper.translate(base + 1).is_some());
        let unchanged = mapper.translate(base + 2).unwrap();
        assert_eq!(unchanged.ppn, existing.ppn);
        assert_eq!(unchanged.flags, existing.flags);
        assert!(mapper.translate(base + 3).is_none());
        assert_eq!(*object.ahead.read(), vec![(4, 8, PageFaultType::Write)]);
    }
}
