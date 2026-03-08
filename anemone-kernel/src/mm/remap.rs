//! Vmalloc & ioremap

use spin::Lazy;

use crate::{
    mm::{
        kpgdir::{kmap, kunmap},
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
/// All IO remapping are uncached for simplicity. TODO: this is currently
/// unimplemented.
///
/// Currently only ioremap is implementeed.
#[derive(Debug)]
struct SysRemaps {
    range_allocator: range_allocator::RangeAllocator<VirtPageRange>,
    io_remapped: BTreeMap<PhysPageNum, IoRemapEntry>,
}

#[derive(Debug, Clone, Copy)]
struct IoRemapEntry {
    phys: PhysPageRange,
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

    fn find_io_overlap(&self, phys_range: PhysPageRange) -> Option<IoRemapEntry> {
        let key = phys_range.start();

        if let Some((_, entry)) = self.io_remapped.range(..=key).next_back() {
            if entry.phys.intersects(&phys_range) {
                return Some(*entry);
            }
        }

        if let Some((_, entry)) = self.io_remapped.range(key..).next() {
            if entry.phys.intersects(&phys_range) {
                return Some(*entry);
            }
        }

        None
    }
}

impl SysRemaps {
    unsafe fn ioremap(&mut self, phys_range: PhysPageRange) -> Result<VirtPageRange, MmError> {
        if self.find_io_overlap(phys_range).is_some() {
            return Err(MmError::AlreadyMapped);
        }

        let npages = phys_range.npages() as usize;
        let virt_range = self.alloc(npages).ok_or(MmError::OutOfMemory)?;

        unsafe {
            kmap(Mapping {
                vpn: virt_range.start(),
                ppn: phys_range.start(),
                npages,
                flags: PteFlags::READ | PteFlags::WRITE,
            })
            .map_err(|e| {
                self.free(virt_range.start(), virt_range.npages() as usize)
                    .expect("internal error: failed to free virt range after failed ioremap");
                e
            })?;
        }

        let prev = self.io_remapped.insert(
            phys_range.start(),
            IoRemapEntry {
                phys: phys_range,
                virt: virt_range,
            },
        );
        assert!(
            prev.is_none(),
            "internal error: duplicated ioremap entry after overlap check"
        );

        Ok(virt_range)
    }

    unsafe fn iounmap(&mut self, range: PhysPageRange) -> Result<(), MmError> {
        let key = range.start();
        let entry = self.io_remapped.get(&key).ok_or(MmError::NotMapped)?;

        if entry.phys != range {
            return Err(MmError::InvalidArgument);
        }

        // Invariant checked above. Remove should always succeed here.
        let entry = self
            .io_remapped
            .remove(&key)
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
    unsafe {
        let remap_region = KernelLayout::REMAP_REGION;
        remaps
            .free(remap_region.start(), remap_region.npages() as usize)
            .expect("failed to initialize remap region");
    }
    SpinLock::new(remaps)
});

#[derive(Debug)]
pub struct IoRemap {
    virt: VirtPageRange,
    phys: PhysPageRange,
}

impl Drop for IoRemap {
    fn drop(&mut self) {
        unsafe {
            SYS_REMAPS
                .lock_irqsave()
                .iounmap(self.phys)
                .expect("failed to iounmap in IoRemap drop");
        }
    }
}

/// Remap the given physical page range to a virtual page range in the remap
/// region, and return the virtual page range.
///
/// # Safety
///
/// Caller must ensure that the given physical page range is a valid MMIO
/// region, and that the caller has exclusive access to the given physical page
/// range.
pub unsafe fn ioremap(range: PhysPageRange) -> Result<IoRemap, MmError> {
    unsafe {
        let virt = SYS_REMAPS.lock_irqsave().ioremap(range)?;
        Ok(IoRemap { virt, phys: range })
    }
}

pub fn vmalloc(npages: usize) -> Option<Todo> {
    todo!()
}
