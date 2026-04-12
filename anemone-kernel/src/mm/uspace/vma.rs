//! Virtual memory area.
//!
//! Reference:
//! - https://fuchsia.dev/fuchsia-src/reference/kernel_objects/vm_address_region

use crate::prelude::{
    vmo::{VmObject, shadow::ShadowObject},
    *,
};

/// Virtual memory area, within a [UserSpace].
#[derive(Debug)]
pub struct VmArea {
    /// `range` along with `poffset` determines where this VMA views into the
    /// underlying [VmObject].
    range: VirtPageRange,
    /// Starting frame index in underlying [VmObject] that corresponds to
    /// `range.start()`.
    poffset: usize,
    /// Permission of this VMA.
    ///
    /// This maybe more than the actual permission of the mapped page.
    perm: PteFlags,
    /// The underlying virtual memory object.
    backing: Arc<RwLock<dyn VmObject>>,
}

impl VmArea {
    pub fn new(
        range: VirtPageRange,
        poffset: usize,
        perm: PteFlags,
        backing: Arc<RwLock<dyn VmObject>>,
    ) -> Self {
        Self {
            range,
            poffset,
            perm,
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
    pub fn set_backing(&mut self, new: Arc<RwLock<dyn VmObject>>) {
        self.backing = new;
    }

    /// Get the permission of this VMA.
    pub fn perm(&self) -> PteFlags {
        self.perm
    }

    /// Get the underlying virtual memory object of this VMA.
    pub fn backing(&self) -> &Arc<RwLock<dyn VmObject>> {
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
    ) -> Result<(), MmError> {
        debug_assert!(self.range.contains(vpn));

        let access_perm = match access {
            PageFaultType::Read => PteFlags::READ,
            PageFaultType::Write => PteFlags::WRITE,
            PageFaultType::Execute => PteFlags::EXECUTE,
        };
        if !self.perm.contains(access_perm) {
            return Err(MmError::PermissionDenied);
        }

        let pidx = self.vmo_pidx(vpn);
        let resolved = self.backing.write().resolve_frame(pidx, access)?;
        let mut flags = self.perm | PteFlags::USER;
        if !resolved.writable {
            flags -= PteFlags::WRITE;
        }

        unsafe { mapper.map_one(vpn, resolved.frame.ppn(), flags, 0, true) }
    }

    /// Handle a page fault in this VMA.
    ///
    /// Address of faulting page is guaranteed to be in the range of this VMA.
    pub fn handle_page_fault(
        &mut self,
        mapper: &mut Mapper,
        fault_info: &PageFaultInfo,
    ) -> Result<(), MmError> {
        let vpn = fault_info.fault_addr().page_down();
        debug_assert!(self.range.contains(vpn));

        /*knoticeln!(
            "handle page fault in VMA: addr={}, access={:?}",
            fault_info.fault_addr(),
            fault_info.fault_type()
        );*/

        self.map_page(mapper, vpn, fault_info.fault_type())?;
        PagingArch::tlb_shootdown(fault_info.fault_addr().page_down());

        Ok(())
    }

    /// Create a shadow copy of this [VmArea].
    ///
    /// Reading from the shadow will read from the original, but writing to the
    /// shadow will not affect the original.
    pub fn shadow(&self) -> VmArea {
        Self {
            range: self.range,
            poffset: self.poffset,
            perm: self.perm,
            backing: Arc::new(RwLock::new(ShadowObject::new(self.backing.clone()))),
        }
    }

    /// Used when cloning a task without `CLONE_VM`. Think of it as forking a
    /// repository: the child process gets a copy of the parent's memory, but
    /// changes in one process won't affect the other...
    ///
    /// Internally, this will create to [ShadowObject]s both pointing to
    /// original backing. Then one of them will back this [VmArea], and the
    /// other will be returned for the child task to use.
    ///
    /// Judging by its name, you might never guess that this function performs
    /// such an important work. So ***be careful***!
    pub fn fork(&mut self, mapper: &mut Mapper) -> Self {
        if self.perm.contains(PteFlags::WRITE) {
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
        self.backing = Arc::new(RwLock::new(pshadow));
        Self {
            range: self.range,
            poffset: self.poffset,
            perm: self.perm,
            backing: Arc::new(RwLock::new(cshadow)),
        }
    }
}
