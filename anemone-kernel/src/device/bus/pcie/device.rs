use core::fmt::Debug;

use kernel_macros::{Device, KObject};
use range_allocator::{IncreasingRangeAllocator, Rangable};

use crate::{
    device::{
        DeviceBase, DeviceOps,
        bus::{
            BusType,
            pcie::{
                HOST_BRIDGE_CLASSCODE, OfPciAddr, PCIE_BUS_TYPE, PciAddrFlags, PciSpaceType,
                bus::preinit_pcie_dev,
                ecam::{
                    BAR, BusNum, ClassCode, DevNum, EcamConf, FuncNum, MemBARType, PcieDeviceConf,
                },
            },
        },
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
    },
    prelude::*,
};

/// Represent a PCIe device, either a bus or an endpoint.
#[derive(Debug, KObject, Device)]
pub struct PcieDevice {
    #[kobject]
    kobj_base: KObjectBase,
    #[device]
    dev_base: DeviceBase,

    /// `domain` PCIe domain owning this device.
    domain: Arc<PcieDomain>,

    /// `typed_info` Detailed PCIe topology metadata and type for this device.
    typed_info: PcieDeviceType,
}

#[derive(Debug)]
pub struct PcieDomain {
    ///  Unique PCIe domain identifier.
    domain: usize,
    /// ECAM configuration used to access PCIe config space.
    conf: EcamConf,
    /// Track latest allocated bus number in this domain.
    bus_num_allocator: AtomicU8,
    io_area: Option<AvailPciMemArea>,
    mem_area_pref: Option<AvailPciMemArea>,
    mem_area_unpref: Option<AvailPciMemArea>,
}

impl PcieDomain {
    /// Create a PCIe domain from a domain id and ECAM configuration.
    ///
    /// `domain` Unique domain identifier.
    /// `ecam` ECAM configuration providing config-space addressing.
    pub fn new(domain: usize, ecam: EcamConf) -> Self {
        Self {
            domain,
            bus_num_allocator: AtomicU8::new(ecam.root_bus_num().into()),
            conf: ecam,
            io_area: None,
            mem_area_pref: None,
            mem_area_unpref: None,
        }
    }

    /// Add a memory range to the domain's resource apertures.
    ///
    /// Ignore config-space apertures. Return `AlreadyMapped` when an aperture
    /// of the same type is already registered.
    pub fn add_mem_range(&mut self, mem_range: AvailPciMemArea) -> Result<(), SysError> {
        if let PciSpaceType::Config = mem_range.pci_addr.space_type() {
            return Ok(());
        } else if let PciSpaceType::IO = mem_range.pci_addr.space_type() {
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

    /// Allocate a memory region for the specified `BAR` from compatible
    /// apertures.
    ///
    /// Return the aperture and allocated area on success.
    pub fn alloc_mem_for_bar(&self, bar: BAR, size: u64) -> Option<(&AvailPciMemArea, PciMemArea)> {
        let mem_ranges_iter: &mut dyn Iterator<Item = &AvailPciMemArea> = match bar {
            BAR::Memory {
                prefetchable: false,
                ..
            } => &mut self.mem_area_unpref.iter(),
            BAR::Memory {
                prefetchable: true, ..
            } => &mut self.mem_area_unpref.iter().chain(self.mem_area_pref.iter()),
            BAR::IO { .. } => &mut self.io_area.iter(),
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
    pub fn snapshot(&self, align: PciMemAreaSnapshot) -> Option<PciMemAreaSnapshot> {
        Some(PciMemAreaSnapshot {
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

    /// Return domain identifier.
    pub fn id(&self) -> usize {
        self.domain
    }

    /// Return ECAM configuration bound to this domain.
    pub fn ecam(&self) -> &EcamConf {
        &self.conf
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
                self.conf.max_bus_num()
            );
            e
        })?;
        self.bus_num_allocator
            .store(new_bus_num.into(), Ordering::SeqCst);
        Ok(new_bus_num)
    }

    /// Return the current maximum allocated bus number.
    pub fn current_max_bus_num(&self) -> BusNum {
        BusNum::try_from(self.bus_num_allocator.load(Ordering::SeqCst)).unwrap()
    }
}

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
    pub fn compatible(&self, bar: BAR) -> bool {
        if self.pci_addr.flags().contains(PciAddrFlags::NotRelocatable) {
            return false;
        }
        if let PciSpaceType::Config = self.pci_addr.space_type() {
            return false;
        }
        match bar {
            BAR::IO { .. } => {
                if let PciSpaceType::IO = self.pci_addr.space_type() {
                    true
                } else {
                    false
                }
            },
            BAR::Memory {
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
                if matches!(mtype, MemBARType::W32)
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

#[derive(Debug)]
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
pub struct PciMemAreaSnapshot {
    pub io_area: Option<u64>,
    pub mem_area_pref: Option<u64>,
    pub mem_area_unpref: Option<u64>,
}

impl Debug for PciMemAreaSnapshot {
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

#[derive(Debug)]
pub enum PcieDeviceType {
    /// Host bridge info.
    HostBridge {
        /// `id` is the root bus number represented by this host bridge.
        id: BusNum,
    },
    /// Bus device, which can have child devices.
    Bus {
        /// `conf` is the device configuration accessor for this bus function.
        conf: PcieDeviceConf,
        /// `id` is the secondary bus number exposed by this bridge.
        id: BusNum,
        /// `bus` is the upstream bus where this bridge function resides.
        bus: BusNum,
        /// `dev` is the device number on the upstream bus.
        dev: DevNum,
    },
    /// Endpoint device, which has no child devices.
    Endpoint {
        /// `conf` is the endpoint's configuration accessor.
        conf: PcieDeviceConf,
        /// `bus` is the bus number where this endpoint resides.
        bus: BusNum,
        /// `dev` is the device number on the bus.
        dev: DevNum,
    },
}

impl KObjectOps for PcieDevice {}

impl DeviceOps for PcieDevice {}

impl PcieDevice {
    /// Return the PCIe configuration accessor when available.
    pub fn dev_conf(&self) -> Option<&PcieDeviceConf> {
        match &self.typed_info {
            PcieDeviceType::Endpoint { conf, .. } => Some(conf),
            PcieDeviceType::Bus { conf, .. } => Some(conf),
            PcieDeviceType::HostBridge { .. } => None,
        }
    }

    /// Return the PCIe domain this device belongs to.
    pub fn domain(&self) -> &Arc<PcieDomain> {
        &self.domain
    }

    /// Return detailed PCIe topology metadata for this device.
    pub fn dev_info(&self) -> &PcieDeviceType {
        &self.typed_info
    }

    /// Return the bus number for bus/endpoint devices.
    pub fn bus_num(&self) -> Option<BusNum> {
        match self.typed_info {
            PcieDeviceType::HostBridge { .. } => None,
            PcieDeviceType::Bus { bus, .. } => Some(bus),
            PcieDeviceType::Endpoint { bus, .. } => Some(bus),
        }
    }

    /// Return the device number for bus/endpoint devices.
    pub fn dev_num(&self) -> Option<DevNum> {
        match self.typed_info {
            PcieDeviceType::HostBridge { .. } => None,
            PcieDeviceType::Bus { dev, .. } => Some(dev),
            PcieDeviceType::Endpoint { dev, .. } => Some(dev),
        }
    }

    /// Read the class code used for driver matching.
    pub fn class_code(&self) -> ClassCode {
        match &self.typed_info {
            PcieDeviceType::HostBridge { .. } => HOST_BRIDGE_CLASSCODE, // Host bridge class code
            PcieDeviceType::Bus { conf, .. } => conf.get_function(FuncNum::MIN).class_code(),
            PcieDeviceType::Endpoint { conf, .. } => conf.get_function(FuncNum::MIN).class_code(),
        }
    }

    pub fn vendor_device_id(&self) -> Option<(u16, u16)> {
        match &self.typed_info {
            PcieDeviceType::HostBridge { .. } => None,
            PcieDeviceType::Bus { conf, .. } => {
                let func = conf.get_function(FuncNum::MIN);
                Some((func.vendor_id(), func.device_id()))
            },
            PcieDeviceType::Endpoint { conf, .. } => {
                let func = conf.get_function(FuncNum::MIN);
                Some((func.vendor_id(), func.device_id()))
            },
        }
    }

    /// Create a PCIe endpoint device object.
    ///
    /// `name` is the device kobject name.
    /// `domain` is the owning PCIe domain.
    /// `bus` is the bus number where this endpoint resides.
    /// `dev` is the device number on `bus`.
    pub fn new_endpoint(
        name: KObjIdent,
        domain: Arc<PcieDomain>,
        bus: BusNum,
        dev: DevNum,
    ) -> Self {
        Self {
            kobj_base: KObjectBase::new(name),
            dev_base: DeviceBase::new(None),
            typed_info: PcieDeviceType::Endpoint {
                bus,
                dev,
                conf: domain.conf.get_bus(bus).get_device(dev),
            },
            domain,
        }
    }

    /// Create a PCIe bus-device object for a bridge function.
    ///
    /// `name` is the device kobject name.
    /// `domain` is the owning PCIe domain.
    /// `bus` is the upstream bus number where this bridge resides.
    /// `dev` is the bridge device number on `bus`.
    /// `id` is the secondary bus number managed by this bridge.
    pub fn new_bus(
        name: KObjIdent,
        domain: Arc<PcieDomain>,
        bus: BusNum,
        dev: DevNum,
        id: BusNum,
    ) -> Self {
        Self {
            kobj_base: KObjectBase::new(name),
            dev_base: DeviceBase::new(None),
            typed_info: PcieDeviceType::Bus {
                id,
                bus,
                dev,
                conf: domain.conf.get_bus(bus).get_device(dev),
            },
            domain,
        }
    }

    /// Create a PCIe host-bridge device object.
    ///
    /// `name` is the device kobject name.
    /// `domain` is the owning PCIe domain.
    /// `id` is the root bus number associated with this host bridge.
    pub fn new_host_bridge(name: KObjIdent, domain: Arc<PcieDomain>, id: BusNum) -> Self {
        Self {
            kobj_base: KObjectBase::new(name),
            dev_base: DeviceBase::new(None),
            domain,
            typed_info: PcieDeviceType::HostBridge { id: id },
        }
    }

    /// Register and add a child device and probe matching PCIe drivers.
    ///
    /// `device` is the child PCIe device to add under `self`.
    pub fn register_and_preinit_device(&self, device: Arc<PcieDevice>) {
        if let PcieDeviceType::Endpoint { .. } = &self.typed_info {
            panic!("cannot register device to an endpoint");
        }
        self.add_child(device.clone());

        for driver in PCIE_BUS_TYPE.base().drivers.read().iter() {
            if PCIE_BUS_TYPE.matches(device.as_ref(), driver.as_ref()) {
                // TODO: probe defer
                if let Err(e) = preinit_pcie_dev(device.as_ref()) {
                    kerrln!(
                        "preinit failed for device {} when probed by driver {}: {:?}",
                        device.name(),
                        driver.name(),
                        e
                    );
                }

                let pcie_driver = driver
                    .as_pcie_driver()
                    .expect("only pcie drivers should be registered to pcie bus");

                if let Err(e) = pcie_driver.postinit(device.clone()) {
                    kerrln!(
                        "postinit failed for device {} when probed by driver {}: {:?}",
                        device.name(),
                        driver.name(),
                        e
                    );
                    return;
                }

                device.set_driver(Some(driver.clone()));

                break;
            }
        }
    }

    pub fn probe_all_devices(&self) {
        //kinfoln!("probing all devices under pcie device {}", self.name());
        (self as &dyn Device).for_each_child(|child| {
            if let Some(driver) = child.driver() {
                /*kinfoln!(
                    "probing device {} with driver {}",
                    child.name(),
                    driver.name()
                );*/
                match driver.probe(child.clone()) {
                    Ok(()) => {
                        driver.attach_device(child.clone());
                    },
                    Err(e) => {
                        child.set_driver(None);
                        kerrln!(
                            "failed to probe device {} with driver {}: {:?}",
                            child.name(),
                            driver.name(),
                            e
                        );
                    },
                }
            }
        });
    }
}
