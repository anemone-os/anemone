//! Vmalloc & ioremap
//!
//! TODO: ioremap - multiplex the same virtual mapping for several requests
//! within the same physical page range

use core::ptr::NonNull;

use crate::{
    mm::{
        kptable::{kmap, kunmap},
        layout::KernelLayoutTrait,
    },
    prelude::*,
};

impl range_allocator::Rangable for VirtPageRange {
    fn start(&self) -> usize {
        self.start().get() as usize
    }

    fn len(&self) -> usize {
        self.npages() as usize
    }

    fn from_parts(start: usize, length: usize) -> Self {
        Self::new(VirtPageNum::new(start as u64), length as u64)
    }
}

/// System state related to virtual memory remapping, including vmalloc and
/// ioremap.
///
/// All IO remapping are uncached for simplicity.
///
/// Currently only ioremap is implementeed.
#[derive(Debug)]
struct SysRemaps {
    range_allocator: range_allocator::RangeAllocator<VirtPageRange>,

    // IoRemap
    io_remapped: BTreeMap<PhysAddr, IoRemapEntry>,
    // VMalloc
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IoRange {
    start: PhysAddr,
    len: u64,
}

impl IoRange {
    fn try_new(start: PhysAddr, len: usize) -> Result<Self, MmError> {
        if len == 0 {
            return Err(MmError::InvalidArgument);
        }
        let len = len as u64;
        start
            .get()
            .checked_add(len)
            .ok_or(MmError::InvalidArgument)?;
        Ok(Self { start, len })
    }

    fn end(&self) -> PhysAddr {
        PhysAddr::new(
            self.start
                .get()
                .checked_add(self.len)
                .expect("IoRange end overflow must be rejected in try_new"),
        )
    }

    fn intersects(&self, other: &Self) -> bool {
        self.start.get() < other.end().get() && other.start.get() < self.end().get()
    }

    fn to_page_range(&self) -> PhysPageRange {
        let start = self.start.page_down();
        let end = self.end().page_up();
        PhysPageRange::new(start, end.get() - start.get())
    }
}

#[derive(Debug, Clone, Copy)]
struct IoRemapEntry {
    req: IoRange,
    virt: VirtPageRange,
}

impl SysRemaps {
    fn new() -> Self {
        Self {
            range_allocator: range_allocator::RangeAllocator::new(),
            io_remapped: BTreeMap::new(),
        }
    }

    fn alloc(&mut self, npages: usize) -> Option<VirtPageRange> {
        self.range_allocator.allocate(npages)
    }

    fn free(&mut self, start: VirtPageNum, npages: usize) -> Result<(), MmError> {
        let range = VirtPageRange::new(start, npages as u64);
        self.range_allocator
            .free(range)
            .map_err(|_| MmError::InvalidArgument)
    }

    fn find_io_overlap(&self, req: IoRange) -> Option<IoRemapEntry> {
        if let Some((_, entry)) = self.io_remapped.range(..=req.start).next_back() {
            if entry.req.intersects(&req) {
                return Some(*entry);
            }
        }

        if let Some((_, entry)) = self.io_remapped.range(req.start..).next() {
            if entry.req.intersects(&req) {
                return Some(*entry);
            }
        }

        None
    }
}

impl SysRemaps {
    unsafe fn ioremap(
        &mut self,
        req: IoRange,
    ) -> Result<(VirtAddr, VirtPageRange, IpiGuard), MmError> {
        if self.find_io_overlap(req).is_some() {
            return Err(MmError::AlreadyMapped);
        }

        let phys_range = req.to_page_range();
        let npages = phys_range.npages() as usize;
        let virt_range = self.alloc(npages).ok_or(MmError::OutOfMemory)?;
        let guard = unsafe {
            kmap(Mapping {
                vpn: virt_range.start(),
                ppn: phys_range.start(),
                npages,
                flags: PteFlags::READ | PteFlags::WRITE | PteFlags::NONCACHE | PteFlags::STRONG,
                huge_pages: false,
            })
            .map_err(|e| {
                self.free(virt_range.start(), virt_range.npages() as usize)
                    .expect("internal error: failed to free virt range after failed ioremap");
                e
            })?
        };

        assert!(
            self.io_remapped
                .insert(
                    req.start,
                    IoRemapEntry {
                        req,
                        virt: virt_range,
                    },
                )
                .is_none(),
            "internal error: duplicated ioremap entry after overlap check"
        );

        let vaddr = virt_range.start().to_virt_addr() + req.start.page_offset() as u64;

        Ok((vaddr, virt_range, guard))
    }

    unsafe fn iounmap(&mut self, req: IoRange) -> Result<(), MmError> {
        let entry = self.io_remapped.get(&req.start).ok_or(MmError::NotMapped)?;

        if entry.req != req {
            return Err(MmError::InvalidArgument);
        }

        // Invariant checked above. Remove should always succeed here.
        let entry = self
            .io_remapped
            .remove(&req.start)
            .expect("internal error: ioremap entry disappeared during iounmap");

        unsafe {
            kunmap(Unmapping { range: entry.virt });
        }

        self.free(entry.virt.start(), entry.virt.npages() as usize)
            .expect("internal error: failed to release virtual range during iounmap");

        Ok(())
    }
}

static SYS_REMAPS: Lazy<SpinLock<SysRemaps>> = Lazy::new(|| {
    let mut remaps = SysRemaps::new();
    let remap_region = KernelLayout::REMAP_REGION;
    remaps
        .free(remap_region.start(), remap_region.npages() as usize)
        .expect("failed to initialize remap region");
    SpinLock::new(remaps)
});

#[derive(Debug)]
pub struct IoRemap {
    virt: VirtAddr,
    req: IoRange,
}

impl IoRemap {
    /// This is how drivers should access the remapped IO region.
    ///
    /// # Safety
    ///
    /// Caller must ensure that the remapped MMIO region is valid for the
    /// intended typed access and respects device-specific ordering.
    pub fn as_ptr(&self) -> NonNull<[u8]> {
        unsafe {
            NonNull::new_unchecked(core::ptr::slice_from_raw_parts_mut(
                self.virt.as_ptr_mut(),
                self.req.len as usize,
            ))
        }
    }
}

impl Drop for IoRemap {
    fn drop(&mut self) {
        unsafe {
            SYS_REMAPS
                .lock_irqsave()
                .iounmap(self.req)
                .expect("failed to iounmap in IoRemap drop");
        }
    }
}

/// Remap the given physical byte range to a virtual byte range in the remap
/// region, and return the driver-visible mapping.
///
/// # Safety
///
/// Caller must ensure that the given physical byte range is a valid MMIO
/// region, and that the caller has exclusive access to that region.
pub unsafe fn ioremap(start: PhysAddr, len: usize) -> Result<IoRemap, MmError> {
    unsafe {
        let req = IoRange::try_new(start, len)?;
        let (virt, _, guard) = SYS_REMAPS.lock_irqsave().ioremap(req)?;
        drop(guard); // send ipi
        Ok(IoRemap { virt, req })
    }
}

pub fn vmalloc(npages: usize) -> Option<Todo> {
    todo!()
}

/// Allocate a contiguous virtual page range from the remap region.
///
/// This is used for dynamic kernel mappings such as stack guard pages.
/// The caller is responsible for mapping the allocated range.
pub unsafe fn alloc_virt_range(npages: usize) -> Option<VirtPageRange> {
    SYS_REMAPS.lock_irqsave().alloc(npages)
}

/// Free a virtual page range previously allocated by [`alloc_virt_range`].
pub unsafe fn free_virt_range(start: VirtPageNum, npages: usize) -> Result<(), MmError> {
    SYS_REMAPS.lock_irqsave().free(start, npages)
}
