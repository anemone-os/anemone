//! System V IPC shared memory implementation.

use crate::{
    mm::uspace::{
        mmap::ObjectMapping,
        vma::{ForkPolicy, Protection, VmFlags},
        vmo::VmObject,
    },
    prelude::*,
};

pub const SHMMIN: usize = 1;
pub const SHMMAX: usize = crate::kconfig_defs::SHMMAX;
pub const SHMALL: usize = crate::kconfig_defs::SHMALL;
pub const SHMMNI: usize = crate::kconfig_defs::SHMMNI;
pub const SHMLBA: usize = PagingArch::PAGE_SIZE_BYTES;
pub const SHMSEG: usize = SHMMNI;

pub const SHM_DEST: u16 = 0o1000;
pub const SHM_LOCKED: u16 = 0o2000;

mod api;
mod object;
mod permission;
mod registry;
mod segment;

pub use api::*;
pub(super) use object::ShmObject;
pub(super) use segment::{ShmAttachment, ShmSegment};

pub(super) fn detach_attachment(attachment: &ShmAttachment, tgid: Tid) {
    attachment.segment.on_detach(tgid);
    if attachment.segment.is_reclaimable() {
        registry::with_registry(|registry| registry.release(attachment.segment.clone()));
    }
}

impl UserSpace {
    fn validate_sysv_remap_range(&self, range: VirtPageRange) -> Result<(), SysError> {
        for attachment in self.sysv_shm.values() {
            let attachment_range = attachment.range();
            if attachment_range.intersects(&range) && !range.covers(&attachment_range) {
                // Avoid losing bookkeeping for a still partially mapped SysV
                // attachment. Generic munmap tracking is intentionally deferred.
                return Err(SysError::InvalidArgument);
            }
        }
        Ok(())
    }

    fn detach_sysv_shm_covered_by(&mut self, range: VirtPageRange, tgid: Tid) {
        let starts: Vec<VirtPageNum> = self
            .sysv_shm
            .iter()
            .filter_map(|(start, attachment)| range.covers(&attachment.range()).then_some(*start))
            .collect();

        for start in starts {
            let attachment = self
                .sysv_shm
                .remove(&start)
                .expect("collected SysV shm attachment must still exist");
            detach_attachment(&attachment, tgid);
        }
    }

    fn attach_sysv_shm(
        &mut self,
        reservation: segment::ShmAttachReservation,
        hint: Option<(VirtPageNum, bool)>,
        clobber: bool,
        prot: Protection,
        tgid: Tid,
    ) -> Result<(VirtAddr, Option<RemoteUspFenceGuard>), SysError> {
        let segment = reservation.segment().clone();
        let npages = segment.npages();
        let remap_range = hint
            .filter(|(_, fixed)| clobber && *fixed)
            .map(|(start, _)| VirtPageRange::new(start, npages as u64));

        if let Some(range) = remap_range {
            if let Err(err) = self.validate_sysv_remap_range(range) {
                let segment = reservation.cancel();
                if segment.is_reclaimable() {
                    registry::with_registry(|registry| registry.release(segment));
                }
                return Err(err);
            }
        }

        let backing: Arc<dyn VmObject> = segment.object();
        let mapping = ObjectMapping {
            hint,
            clobber,
            npages,
            prot,
            on_fork: ForkPolicy::Shared,
            flags: VmFlags::empty(),
            poffset: 0,
            backing,
        };
        let (addr, guard) = match self.map_object(&mapping) {
            Ok(mapped) => mapped,
            Err(err) => {
                let segment = reservation.cancel();
                if segment.is_reclaimable() {
                    registry::with_registry(|registry| registry.release(segment));
                }
                return Err(err);
            },
        };

        if let Some(range) = remap_range {
            self.detach_sysv_shm_covered_by(range, tgid);
        }

        let start = addr.page_down();
        let old = self.sysv_shm.insert(
            start,
            ShmAttachment {
                segment: segment.clone(),
                start,
                prot,
            },
        );
        assert!(old.is_none(), "SysV shm attach start must be unique");
        reservation.commit(tgid);

        Ok((addr, guard))
    }

    fn detach_sysv_shm_at(
        &mut self,
        start: VirtPageNum,
        tgid: Tid,
    ) -> Result<RemoteUspFenceGuard, SysError> {
        let attachment = self
            .sysv_shm
            .get(&start)
            .cloned()
            .ok_or(SysError::InvalidArgument)?;
        let range = attachment.range();
        let guard = self.unmap(range)?;

        self.sysv_shm
            .remove(&start)
            .expect("validated SysV shm attachment must still exist");
        detach_attachment(&attachment, tgid);

        Ok(guard)
    }
}
