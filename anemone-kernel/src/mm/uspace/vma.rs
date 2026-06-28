//! Virtual memory area.
//!
//! Reference:
//! - https://fuchsia.dev/fuchsia-src/reference/kernel_objects/vm_address_region

use crate::prelude::{
    vmo::{VmObject, shadow::ShadowObject},
    *,
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
/// to [VmArea::handle_page_fault]. We should refactor some APIs to forbid those
/// invalid usage later.
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

    fn map_page(
        &mut self,
        mapper: &mut Mapper,
        vpn: VirtPageNum,
        access: PageFaultType,
    ) -> Result<(), SysError> {
        debug_assert!(self.range.contains(vpn));

        if !self.prot.contains(access.into()) {
            return Err(SysError::PermissionDenied);
        }

        let pidx = self.vmo_pidx(vpn);
        let resolved = self.backing.resolve_frame(pidx, access)?;
        let mut flags: PteFlags = PteFlags::from(self.prot) | PteFlags::USER;
        if !resolved.writable {
            flags -= PteFlags::WRITE;
        }

        unsafe { mapper.map_one(vpn, resolved.frame.ppn(), flags, 0, true) }
    }

    /// Handle a page fault in this VMA.
    ///
    /// Address of faulting page is guaranteed to be in the range of this VMA.
    ///
    /// A local TLB shootdown will be performed.
    pub(super) fn handle_page_fault(
        &mut self,
        mapper: &mut Mapper,
        fault_info: &PageFaultInfo,
    ) -> Result<(), SysError> {
        let vpn = fault_info.fault_addr().page_down();
        debug_assert!(self.range.contains(vpn));

        self.map_page(mapper, vpn, fault_info.fault_type())?;
        PagingArch::tlb_shootdown(fault_info.fault_addr().page_down());

        Ok(())
    }

    /// Used when cloning a task without `CLONE_VM`. Think of it as forking a
    /// repository: the child process gets a copy of the parent's memory, but
    /// changes in one process won't affect the other...
    ///
    /// See [ForkPolicy] for more details about how the forking is performed.
    ///
    /// Judging by its name, you might never guess that this function performs
    /// such an important work. So ***be careful***!
    ///
    /// A local tlb shootdown will be performed when necessary (i.e. when
    /// [ForkPolicy::CopyOnWrite] and we need to remove write permissions).
    pub(super) fn fork(&mut self, mapper: &mut Mapper) -> Self {
        match self.on_fork {
            ForkPolicy::Shared => Self {
                range: self.range,
                poffset: self.poffset,
                prot: self.prot,
                on_fork: self.on_fork,
                flags: self.flags,
                reservation: self.reservation,
                backing: self.backing.clone(),
            },
            ForkPolicy::CopyOnWrite => {
                if self.prot.contains(Protection::WRITE) {
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
                    PagingArch::tlb_shootdown_all();
                }

                let original = self.backing.clone();

                let pshadow = ShadowObject::new(original.clone());
                let cshadow = ShadowObject::new(original.clone());
                self.backing = Arc::new(pshadow);
                Self {
                    range: self.range,
                    poffset: self.poffset,
                    prot: self.prot,
                    on_fork: self.on_fork,
                    flags: self.flags,
                    reservation: self.reservation,
                    backing: Arc::new(cshadow),
                }
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
