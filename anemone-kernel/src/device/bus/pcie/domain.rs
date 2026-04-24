use core::{
    fmt::Debug,
    ops::BitAnd,
    sync::atomic::{AtomicU8, AtomicUsize, Ordering},
};

use alloc::collections::btree_map::BTreeMap;
use range_allocator::{IncreasingRangeAllocator, Rangable};

use crate::{
    device::{
        bus::pcie::{
            OfPciAddr, PciAddrFlags, PciFuncAddr, PciSpaceType,
            ecam::{BusNum, EcamConf, PciBar, PciMemBarType},
        },
        discovery::fwnode::FwNode,
    },
    prelude::*,
};

/// Represent a PCIe domain.
///
/// A PCIe domain is a collection of PCIe devices that are managed together,
/// with one PCIe host bridge as the root.
///
/// In device trees, a PCIe domain is represented by a node with compatible
/// string "pci-host-ecam-generic" and properties describing the ECAM
/// configuration and resource apertures.
#[derive(Debug)]
pub struct PcieDomain {
    ///  Unique PCIe domain identifier.
    id: usize,
    /// ECAM configuration used to access PCIe config space.
    conf_space: EcamConf,
    /// Resource management for this domain.
    resources: PcieResources,
}

impl PcieDomain {
    /// Create a PCIe domain ECAM configuration.
    ///
    /// A unique domain identifier is automatically generated for each created
    /// domain.
    ///
    /// `ecam` ECAM configuration providing config-space addressing.
    pub fn new(ecam: EcamConf, root_bus_num: BusNum, max_bus_num: BusNum) -> Self {
        static DOMAIN_ID_ALLOC: AtomicUsize = AtomicUsize::new(0);
        Self {
            id: DOMAIN_ID_ALLOC.fetch_add(1, Ordering::SeqCst),
            conf_space: ecam,
            resources: PcieResources::new(root_bus_num, max_bus_num),
        }
    }

    /// Return a reference to this domain's resource management.
    pub fn resources(&self) -> &PcieResources {
        &self.resources
    }

    /// Return a mutable reference to this domain's resource management.
    ///
    /// It's only available during PCIe domain initialization.
    pub fn resources_mut(&mut self) -> &mut PcieResources {
        &mut self.resources
    }

    /// Return domain identifier.
    pub fn id(&self) -> usize {
        self.id
    }

    /// Return ECAM configuration bound to this domain.
    pub fn ecam(&self) -> &EcamConf {
        &self.conf_space
    }
}

/// Represent PCIe resource management for a domain, including bus number
/// allocation, memory and I/O apertures, and interrupt mappings.
#[derive(Debug)]
pub struct PcieResources {
    bus_num_allocator: AtomicU8,
    max_bus_num: BusNum,
    io_area: Option<AvailPciMemArea>,
    mem_area_pref: Option<AvailPciMemArea>,
    mem_area_unpref: Option<AvailPciMemArea>,
    intr_map: BTreeMap<PcieIntrKey, PcieIntrInfo>,
    intr_key_mask: Option<PcieIntrKey>,
}

impl PcieResources {
    pub fn new(root_bus_num: BusNum, max_bus_num: BusNum) -> Self {
        Self {
            bus_num_allocator: AtomicU8::new(root_bus_num.into()),
            io_area: None,
            mem_area_pref: None,
            mem_area_unpref: None,
            intr_map: BTreeMap::new(),
            intr_key_mask: None,
            max_bus_num,
        }
    }

    /// Get the max bus num supported for this resources set
    pub fn max_bus_num(&self) -> BusNum {
        self.max_bus_num
    }

    /// Add a memory range to the domain's resource apertures.
    ///
    /// These [AvailPciMemArea]s are ignored:
    ///  * Config space apertures,
    ///  * Not-relocatable apertures
    ///
    /// According to PCIe & device tree specifications,
    /// [PciAddrFlags::Special] should not appear in a `range` property of a
    /// PCIe host bridge node.
    pub fn add_mem_range(&mut self, mem_range: AvailPciMemArea) -> Result<(), SysError> {
        if let PciSpaceType::Config = mem_range.pci_addr.space_type() {
            return Ok(());
        }
        if mem_range
            .pci_addr
            .flags()
            .contains(PciAddrFlags::NotRelocatable)
        {
            return Ok(());
        }

        debug_assert!(!mem_range.pci_addr.flags().contains(PciAddrFlags::Special));

        if let PciSpaceType::IO = mem_range.pci_addr.space_type() {
            match &self.io_area {
                Some(_) => return Err(SysError::AlreadyMapped),
                None => self.io_area = Some(mem_range),
            }
        } else if mem_range
            .pci_addr
            .flags()
            .contains(PciAddrFlags::Prefetchable)
        {
            match &self.mem_area_pref {
                Some(_) => return Err(SysError::AlreadyMapped),
                None => self.mem_area_pref = Some(mem_range),
            }
        } else {
            match &self.mem_area_unpref {
                Some(_) => return Err(SysError::AlreadyMapped),
                None => self.mem_area_unpref = Some(mem_range),
            }
        }
        Ok(())
    }

    /// Add an interrupt mapping for this domain.
    pub fn add_intr_map(&mut self, key: PcieIntrKey, intr_info: PcieIntrInfo) {
        self.intr_map.insert(key, intr_info);
    }

    /// Set a mask to apply to interrupt keys when looking up interrupt mapping
    /// information.
    pub fn set_intr_key_mask(&mut self, mask: PcieIntrKey) {
        self.intr_key_mask = Some(mask);
    }

    /// Find interrupt mapping information for the given key
    pub fn find_intr_info(&self, mut key: PcieIntrKey) -> Option<&PcieIntrInfo> {
        if let Some(key_mask) = self.intr_key_mask {
            key = key & key_mask;
        }
        self.intr_map.get(&key)
    }

    /// Allocate a memory region for the specified `BAR` from compatible
    /// apertures.
    ///
    /// Return the aperture and allocated area on success.
    pub fn alloc_mem_for_bar(
        &self,
        bar: PciBar,
        size: u64,
    ) -> Option<(&AvailPciMemArea, PciMemArea)> {
        let mem_ranges_iter: &mut dyn Iterator<Item = &AvailPciMemArea> = match bar {
            PciBar::Memory {
                prefetchable: false,
                ..
            } => &mut self.mem_area_unpref.iter(),
            PciBar::Memory {
                prefetchable: true, ..
            } => &mut self.mem_area_unpref.iter().chain(self.mem_area_pref.iter()),
            PciBar::IO { .. } => &mut self.io_area.iter(),
        };
        while let Some(next) = mem_ranges_iter.next() {
            if next.compatible(bar) {
                if let Some(addr) = next.alloc(size) {
                    return Some((next, addr));
                }
            }
        }
        None
    }

    /// Snapshot current allocated addresses in each aperture, aligned up to
    /// `align`.
    pub fn snapshot_mems(&self, align: PcieMemAreaSnapshot) -> Option<PcieMemAreaSnapshot> {
        Some(PcieMemAreaSnapshot {
            io_area: self.io_area.as_ref().and_then(|area| {
                area.allocator
                    .write()
                    .align_current_to(align.io_area.unwrap_or(1) as usize)
                    .and_then(|x| Some(x as u64))
            }),
            mem_area_pref: self.mem_area_pref.as_ref().and_then(|area| {
                area.allocator
                    .write()
                    .align_current_to(align.mem_area_pref.unwrap_or(1) as usize)
                    .and_then(|x| Some(x as u64))
            }),
            mem_area_unpref: self.mem_area_unpref.as_ref().and_then(|area| {
                area.allocator
                    .write()
                    .align_current_to(align.mem_area_unpref.unwrap_or(1) as usize)
                    .and_then(|x| Some(x as u64))
            }),
        })
    }

    /// Allocate the next available bus number in this domain.
    ///
    /// Return a `SysError` when bus number allocation would overflow.
    pub fn alloc_bus_num(&self) -> Result<BusNum, SysError> {
        let bus_num_u8 = self.bus_num_allocator.load(Ordering::SeqCst);
        let next = bus_num_u8.checked_add(1).ok_or_else(|| {
            kerrln!("Error allocating bus number: the bus number exceeds 255.");
            SysError::InvalidArgument
        })?;
        let new_bus_num = BusNum::try_from(next).map_err(|e| {
            kerrln!(
                "Error allocating bus number: the bus number '{}' exceeds the max value '{:?}'.",
                next,
                self.max_bus_num
            );
            e
        })?;
        self.bus_num_allocator
            .store(new_bus_num.into(), Ordering::SeqCst);
        Ok(new_bus_num)
    }

    /// Return the current bus number allocation cursor, which is the next bus
    /// number to be allocated.
    pub fn current_bus_num(&self) -> BusNum {
        BusNum::try_from(self.bus_num_allocator.load(Ordering::SeqCst)).unwrap()
    }
}

// region: PCIe interrupt mapping

#[derive(Debug, Clone)]
pub struct PcieIntrInfo {
    pub parent: Arc<dyn FwNode>,
    pub parent_intr_spec: Box<[u8]>,
}

impl PartialEq for PcieIntrInfo {
    fn eq(&self, other: &Self) -> bool {
        self.parent_intr_spec == other.parent_intr_spec && self.parent.equals(other.parent.as_ref())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PcieIntrKey {
    pub func_addr: PciFuncAddr,
    pub intr_pin: u8,
}

impl BitAnd for PcieIntrKey {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self {
            func_addr: self.func_addr & rhs.func_addr,
            intr_pin: self.intr_pin & rhs.intr_pin,
        }
    }
}

// endregion

// region: PCIe memory area

#[derive(Debug)]
pub struct AvailPciMemArea {
    pci_addr: OfPciAddr,
    phys_addr: PhysAddr,
    size: u64,
    allocator: RwLock<IncreasingRangeAllocator<PciAddrRange>>,
}

impl AvailPciMemArea {
    pub fn new(pci_addr: OfPciAddr, mem_addr: PhysAddr, size: u64) -> Self {
        let mut alloc = IncreasingRangeAllocator::<PciAddrRange>::new(PciAddrRange {
            start: pci_addr.address(),
            end: pci_addr.address() + size,
        });
        Self {
            pci_addr,
            phys_addr: mem_addr,
            size,
            allocator: RwLock::new(alloc),
        }
    }

    /// Free a previously allocated area back to this aperture.
    ///
    /// # Safety
    /// PCIe uses an incremental-only allocation strategy. Free only recently
    /// allocated ranges; otherwise, the allocator may not restore memory
    /// correctly.
    pub unsafe fn free(&self, area: PciMemArea) {
        let mut alloc = self.allocator.write();
        alloc.free(PciAddrRange {
            start: area.pci_addr.address(),
            end: area.pci_addr.address() + area.size,
        });
    }

    /// Allocate a `PciMemArea` of `size` from this aperture.
    pub fn alloc(&self, size: u64) -> Option<PciMemArea> {
        let mut alloc = self.allocator.write();
        if let Some(range) = alloc.allocate_aligned(size as usize, size as usize) {
            let offset = range.start - self.pci_addr.address();
            Some(PciMemArea {
                pci_addr: self.pci_addr + offset,
                phys_addr: self.phys_addr + offset,
                size,
            })
        } else {
            None
        }
    }

    /// Return whether this aperture is compatible with `bar`.
    pub fn compatible(&self, bar: PciBar) -> bool {
        if self.pci_addr.flags().contains(PciAddrFlags::NotRelocatable) {
            return false;
        }
        if let PciSpaceType::Config = self.pci_addr.space_type() {
            return false;
        }
        match bar {
            PciBar::IO { .. } => {
                if let PciSpaceType::IO = self.pci_addr.space_type() {
                    true
                } else {
                    false
                }
            },
            PciBar::Memory {
                mtype,
                prefetchable,
                ..
            } => {
                if let PciSpaceType::IO = self.pci_addr.space_type() {
                    return false;
                }
                if !prefetchable && self.pci_addr.flags().contains(PciAddrFlags::Prefetchable) {
                    return false;
                }
                if matches!(mtype, PciMemBarType::W32)
                    && self.pci_addr.space_type() == PciSpaceType::Mem64
                {
                    return false;
                }
                true
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PciAddrRange {
    start: u64,
    end: u64,
}

impl Rangable for PciAddrRange {
    fn start(&self) -> usize {
        self.start as usize
    }

    fn len(&self) -> usize {
        (self.end - self.start) as usize
    }

    fn from_parts(start: usize, length: usize) -> Self {
        Self {
            start: start as u64,
            end: (start + length) as u64,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PciMemArea {
    pci_addr: OfPciAddr,
    phys_addr: PhysAddr,
    size: u64,
}

impl PciMemArea {
    pub fn pci_addr(&self) -> OfPciAddr {
        self.pci_addr
    }

    pub fn phys_addr(&self) -> PhysAddr {
        self.phys_addr
    }

    pub fn size(&self) -> u64 {
        self.size
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PcieMemAreaSnapshot {
    pub io_area: Option<u64>,
    pub mem_area_pref: Option<u64>,
    pub mem_area_unpref: Option<u64>,
}

impl Debug for PcieMemAreaSnapshot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PciMemAreaSnapshot")
            .field("io_area", &format_args!("{:#x}", self.io_area.unwrap_or(0)))
            .field(
                "mem_area_pref",
                &format_args!("{:#x}", self.mem_area_pref.unwrap_or(0)),
            )
            .field(
                "mem_area_unpref",
                &format_args!("{:#x}", self.mem_area_unpref.unwrap_or(0)),
            )
            .finish()
    }
}

// endregion
