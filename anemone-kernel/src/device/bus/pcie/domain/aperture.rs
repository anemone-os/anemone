//! PCIe resource apertures: collections of I/O and memory windows, each backed
//! by an increasing-range allocator, plus the `AvailableApertures` query type
//! used during BAR allocation.

use range_allocator::{IncreasingRangeAllocator, Rangable};

use crate::{
    device::bus::pcie::{
        OfPciAddr, OfPciAddrFlags, PciSpaceType,
        ecam::{PciBar, PciMemBarType},
    },
    prelude::*,
};

/// All apertures of a PCIe domain, grouped by space type, prefetchability,
/// and address width.
#[derive(Debug)]
pub struct PcieApertureSet {
    io: Vec<PcieAperture>,
    /// 32-bit prefetchable memory.
    mem_pref32: Vec<PcieAperture>,
    /// 32-bit non-prefetchable memory.
    mem_unpref32: Vec<PcieAperture>,
    /// 64-bit prefetchable memory.
    mem_pref64: Vec<PcieAperture>,
    /// 64-bit non-prefetchable memory.
    mem_unpref64: Vec<PcieAperture>,
}

impl PcieApertureSet {
    /// Create an empty aperture set.
    pub fn new() -> Self {
        Self {
            io: vec![],
            mem_pref32: vec![],
            mem_unpref32: vec![],
            mem_pref64: vec![],
            mem_unpref64: vec![],
        }
    }

    /// Add an aperture described by `(pci_start, phys_start, length)`.
    ///
    /// Config-space and non-relocatable apertures are silently ignored.
    /// The `t` (Special) flag is rejected per the device-tree binding.
    pub fn add_aperture(
        &mut self,
        pci_start: OfPciAddr,
        phys_start: PhysAddr,
        length: u64,
    ) -> Result<(), SysError> {
        if let PciSpaceType::Config = pci_start.space_type() {
            return Ok(());
        }

        if pci_start.flags().contains(OfPciAddrFlags::Special) {
            kerrln!(
                "error adding aperture to a PCIe domain: 't' bit are not supported in the 'range' property of a PCIe host bridge node."
            );
            return Err(SysError::InvalidArgument);
        }

        if pci_start.flags().contains(OfPciAddrFlags::NotRelocatable) {
            return Ok(());
        }

        let container = match pci_start.space_type() {
            PciSpaceType::Config => unreachable!(),
            PciSpaceType::IO => &mut self.io,
            PciSpaceType::Mem32 => {
                if pci_start.flags().contains(OfPciAddrFlags::Prefetchable) {
                    &mut self.mem_pref32
                } else {
                    &mut self.mem_unpref32
                }
            },
            PciSpaceType::Mem64 => {
                if pci_start.flags().contains(OfPciAddrFlags::Prefetchable) {
                    &mut self.mem_pref64
                } else {
                    &mut self.mem_unpref64
                }
            },
        };

        container.push(PcieAperture {
            pci_addr: pci_start,
            phys_addr: phys_start,
            size: length,
            allocator: RwLock::new(IncreasingRangeAllocator::new(
                PcieApertureRange::from_parts(pci_start.address() as usize, length as usize),
            )),
        });

        Ok(())
    }
}

/// Range `[start, end)` used by the increasing-range allocator.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PcieApertureRange {
    pub start: usize,
    pub end: usize,
}

impl Rangable for PcieApertureRange {
    fn start(&self) -> usize {
        self.start
    }

    fn len(&self) -> usize {
        self.end - self.start
    }

    fn from_parts(start: usize, length: usize) -> Self {
        Self {
            start,
            end: start + length,
        }
    }
}

/// A single PCIe resource window: a contiguous range of PCI bus addresses
/// mapped 1:1 to physical addresses, with an increasing-range allocator inside.
///
/// # Safety
/// The allocator is increasing-only. Freeing a range that is not the most
/// recently allocated will corrupt the allocator state.
#[derive(Debug)]
pub struct PcieAperture {
    pci_addr: OfPciAddr,
    phys_addr: PhysAddr,
    size: u64,
    allocator: RwLock<IncreasingRangeAllocator<PcieApertureRange>>,
}

impl PcieAperture {
    /// Free a previously allocated area.
    ///
    /// # Safety
    /// The increasing-range allocator only supports freeing the most recently
    /// allocated range. Freeing an older range corrupts the allocator.
    pub unsafe fn free(&self, area: PcieMemArea) {
        let mut alloc = self.allocator.write();
        alloc.free(PcieApertureRange::from_parts(
            area.pci_addr.address() as usize,
            area.size as usize,
        ));
    }

    /// Allocate `size` bytes with natural alignment from this aperture.
    pub fn alloc(&self, size: u64) -> Option<PcieMemArea> {
        let mut alloc = self.allocator.write();
        if let Some(range) = alloc.allocate_aligned(size as usize, size as usize) {
            let offset = (range.start as u64) - self.pci_addr.address();
            Some(PcieMemArea {
                pci_addr: self.pci_addr + offset,
                phys_addr: self.phys_addr + offset,
                size,
            })
        } else {
            None
        }
    }

    /// Remaining free space in this aperture.
    pub fn free_size(&self) -> u64 {
        self.allocator.read().free_size() as u64
    }

    /// Align the allocation cursor to `align` and return the new cursor.
    pub fn snapshot_aligned(&self, align: u64) -> Option<u64> {
        let mut alloc = self.allocator.write();
        alloc
            .align_current_to(align as usize)
            .map(|addr| addr as u64)
    }
}

/// A region allocated from an aperture: PCI address, physical address, and
/// size.
#[derive(Debug, Clone)]
pub struct PcieMemArea {
    pci_addr: OfPciAddr,
    phys_addr: PhysAddr,
    size: u64,
}

impl PcieMemArea {
    /// PCI bus address of this region.
    pub fn pci_addr(&self) -> OfPciAddr {
        self.pci_addr
    }

    /// Physical (CPU) address of this region.
    pub fn phys_addr(&self) -> PhysAddr {
        self.phys_addr
    }

    /// Size in bytes.
    pub fn size(&self) -> u64 {
        self.size
    }
}

/// Snapshot of available apertures, grouped by type, used for BAR allocation.
pub struct AvailableApertures<'a> {
    pub io_area: Vec<&'a PcieAperture>,
    pub mem_area_pref32: Vec<&'a PcieAperture>,
    pub mem_area_pref64: Vec<&'a PcieAperture>,
    pub mem_area_unpref32: Vec<&'a PcieAperture>,
    pub mem_area_unpref64: Vec<&'a PcieAperture>,
}

impl<'a> AvailableApertures<'a> {
    /// Collect all apertures from the given set.
    pub fn pick_all_from(set: &'a PcieApertureSet) -> Self {
        Self {
            io_area: set.io.iter().collect(),
            mem_area_pref32: set.mem_pref32.iter().collect(),
            mem_area_pref64: set.mem_pref64.iter().collect(),
            mem_area_unpref32: set.mem_unpref32.iter().collect(),
            mem_area_unpref64: set.mem_unpref64.iter().collect(),
        }
    }

    /// Allocate a memory region matching the BAR type, trying apertures in
    /// priority order (prefetchable before non-prefetchable, 64-bit before
    /// 32-bit when compatible).
    pub fn alloc_mem_for_bar(
        &self,
        bar: PciBar,
        size: u64,
    ) -> Option<(&'a PcieAperture, PcieMemArea)> {
        let mem_range: &mut dyn Iterator<Item = &&PcieAperture> = match bar {
            PciBar::Memory {
                prefetchable: true,
                mtype: PciMemBarType::W64,
                ..
            } => &mut self
                .mem_area_pref64
                .iter()
                .chain(self.mem_area_pref32.iter())
                .chain(self.mem_area_unpref64.iter())
                .chain(self.mem_area_unpref32.iter()),
            PciBar::Memory {
                prefetchable: true,
                mtype: PciMemBarType::W32,
                ..
            } => &mut self
                .mem_area_pref32
                .iter()
                .chain(self.mem_area_unpref32.iter()),
            PciBar::Memory {
                prefetchable: false,
                mtype: PciMemBarType::W64,
                ..
            } => &mut self
                .mem_area_unpref64
                .iter()
                .chain(self.mem_area_unpref32.iter()),
            PciBar::Memory {
                prefetchable: false,
                mtype: PciMemBarType::W32,
                ..
            } => &mut self.mem_area_unpref32.iter(),
            PciBar::IO { .. } => &mut self.io_area.iter(),
        };
        while let Some(next) = mem_range.next() {
            if let Some(addr) = next.alloc(size) {
                return Some((next, addr));
            }
        }
        None
    }
}
