use core::fmt::Debug;

use crate::{
    device::bus::pcie::{
        OfPciAddr,
        domain::{PcieApertureSet, PcieIntrInfo, PcieIntrKey, PcieIntrSet},
        ecam::BusNum,
    },
    prelude::*,
};

/// Resources of a PCIe domain: bus-number allocator, memory/I/O apertures,
/// and interrupt routing table.
#[derive(Debug)]
pub struct PcieResources {
    /// Next available bus number (starts at root bus + 1).
    bus_num_allocator: AtomicU8,
    max_bus_num: BusNum,
    aperture: PcieApertureSet,
    intr_set: PcieIntrSet,
}

impl PcieResources {
    /// Create resources with the given root bus and max bus.
    pub fn new(root_bus_num: BusNum, max_bus_num: BusNum) -> Self {
        Self {
            bus_num_allocator: AtomicU8::new(root_bus_num.into()),
            max_bus_num,
            aperture: PcieApertureSet::new(),
            intr_set: PcieIntrSet::new(),
        }
    }

    /// Maximum bus number supported by this resource set.
    pub fn max_bus_num(&self) -> BusNum {
        self.max_bus_num
    }

    /// Add a `(pci_start, phys_start, length)` memory range.
    ///
    /// Config-space and non-relocatable apertures are silently ignored.
    /// The `Special` flag is rejected per the device-tree binding.
    pub fn add_mem_range(
        &mut self,
        pci_start: OfPciAddr,
        phys_start: PhysAddr,
        length: u64,
    ) -> Result<(), SysError> {
        self.aperture.add_aperture(pci_start, phys_start, length)
    }

    /// Add an interrupt mapping entry.
    pub fn add_intr_map(&mut self, key: PcieIntrKey, intr_info: PcieIntrInfo) {
        self.intr_set.add_intr_map(key, intr_info);
    }

    /// Set the interrupt-key mask for wildcard matching.
    pub fn set_intr_key_mask(&mut self, mask: PcieIntrKey) {
        self.intr_set.set_intr_key_mask(mask);
    }

    /// Look up interrupt info for a key.
    pub fn find_intr_info(&self, mut key: PcieIntrKey) -> Option<&PcieIntrInfo> {
        self.intr_set.find_intr_info(key)
    }

    /// Allocate the next available bus number.
    ///
    /// Returns `SysError` if the bus number would overflow u8 or exceed the
    /// domain's `max_bus_num`.
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

    /// Current allocation cursor (the next bus number to be handed out).
    pub fn current_bus_num(&self) -> BusNum {
        BusNum::try_from(self.bus_num_allocator.load(Ordering::SeqCst)).unwrap()
    }

    /// Reference to the domain's aperture set.
    pub fn aperture(&self) -> &PcieApertureSet {
        &self.aperture
    }
}
