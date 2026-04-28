//! PCIe bridge driver: bus enumeration, resource allocation, and child-device
//! lifecycle.
//!
//! [`BridgeDriver`] matches PCI-to-PCI bridges (0x0604) and host bridges
//! (0x0600). It enumerates functions on the secondary bus, allocates I/O and
//! memory apertures to children, and programs Type-1 base/limit registers.
//!
//! 64-bit memory apertures are **not** supported. On failure, children are torn
//! down in reverse initialization order.

use crate::{
    device::{
        bus::pcie::{
            self, CLASSCODE_BRIDGE, CLASSCODE_HOST_BRIDGE, PciFunctionIdentifier, PcieDevice,
            PcieDeviceType, PcieDriver,
            domain::{AvailableApertures, PcieIntrKey},
            ecam::{
                BusNum, DevNum, DevNumRangeInclusive, PciClassCode, PciCommands, PciHeaderLayout,
                Type0FuncConf, Type1FuncConf,
            },
        },
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
    },
    prelude::*,
};

/// Driver for PCI-to-PCI bridges (class 0x0604) and host bridges (0x0600).
#[derive(Debug, KObject, Driver)]
struct BridgeDriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for BridgeDriver {}

impl DriverOps for BridgeDriver {
    /// Probe the bridge and trigger child-bus enumeration.
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        let pdev = device
            .as_pcie_device()
            .expect("pcie driver should only be probed with pcie device");
        if let Some(conf) = pdev.func_conf() {
            kinfoln!(
                "enabling bridge device at {} by setting command register",
                pdev.name()
            );
            conf.write_command(
                PciCommands::IO_SPACE | PciCommands::MEM_SPACE | PciCommands::BUS_MASTER,
            );
        }
        pdev.probe_all_devices();
        Ok(())
    }

    fn shutdown(&self, _device: &dyn Device) {
        // TODO: implement bridge shutdown
    }

    fn as_pcie_driver(&self) -> Option<&dyn PcieDriver> {
        Some(self as &dyn PcieDriver)
    }
}

impl BridgeDriver {}

impl PcieDriver for BridgeDriver {
    /// Matches PCI-to-PCI bridge (0x0604) and host bridge (0x0600).
    fn class_code_table(&self) -> &'static [PciClassCode] {
        &[CLASSCODE_BRIDGE, CLASSCODE_HOST_BRIDGE]
    }

    /// No vendor/device-ID matching — class-code only.
    fn vendor_device_table(&self) -> &[(u16, u16)] {
        &[]
    }

    /// Enumerate the bus behind this bridge.
    ///
    /// For a Bus-type device the bus is enumerated directly. For an Endpoint
    /// with a sub-bus, enumeration runs on the sub-bus.
    fn preinit(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        let pdev = Arc::downcast::<PcieDevice>(device)
            .expect("pcie driver should only be initialized with pcie device");
        match pdev.dev_info() {
            PcieDeviceType::Bus { id } => {
                enum_pcie_bus(&pdev, id);
            },
            PcieDeviceType::Endpoint {
                sub_bus: Some(sub_bus),
                ..
            } => match sub_bus.dev_info() {
                PcieDeviceType::Bus { id } => enum_pcie_bus(sub_bus, id),
                _ => panic!("invalid sub bus device type for bridge driver"),
            },
            _ => {
                return Err(SysError::DriverIncompatible);
            },
        }
        Ok(())
    }

    /// Allocate apertures to children and program Type-1 base/limit registers.
    ///
    /// For Bus-type: delegates directly to children. For Endpoint with sub-bus:
    /// snapshots the largest apertures before/after children consume, then
    /// programs base/limit registers so the bridge forwards only consumed
    /// ranges.
    ///
    /// I/O is 4KB-aligned; memory is 1MB-aligned. 64-bit prefetchable memory is
    /// **not** supported.
    fn alloc_resources(
        &self,
        device: Arc<dyn Device>,
        resources: &AvailableApertures,
    ) -> Result<(), SysError> {
        let pdev = Arc::downcast::<PcieDevice>(device.clone())
            .expect("pcie driver should only be initialized with pcie device");
        match pdev.dev_info() {
            PcieDeviceType::Bus { .. } => {
                device.for_each_child(|child| {
                    if let Some(driver) = child.driver() {
                        let pdriver = driver
                            .as_pcie_driver()
                            .expect("child of pcie host bridge should be probed with pcie driver");
                        kinfoln!(
                            "allocating resources for pcie bus device {} with driver {}",
                            child.name(),
                            driver.name()
                        );
                        if let Err(e) = pdriver.alloc_resources(child.clone(), resources) {
                            kerrln!(
                                "alloc_resources failed for device {} when probed by driver {}: {:?}",
                                child.name(),
                                driver.name(),
                                e
                            );
                            pdriver.fail(child.as_ref());
                        }
                    }
                });
            },
            PcieDeviceType::Endpoint {
                conf,
                sub_bus: Some(bus_pdev),
                ..
            } => {
                let io_area = resources.io_area.iter().max_by_key(|ap| ap.free_size());
                let mem_area_unpref32 = resources
                    .mem_area_unpref32
                    .iter()
                    .max_by_key(|ap| ap.free_size());
                let mem_area_pref32 = resources
                    .mem_area_pref32
                    .iter()
                    .max_by_key(|ap| ap.free_size());

                let bus_dev = bus_pdev.as_ref() as &dyn Device;

                const IO_ALIGN: u64 = 0x1000; // 4KB alignment for I/O
                const MEM_ALIGN: u64 = 0x100000; // 1MB alignment for memory

                // Snapshot apertures before allocation to establish base addresses.
                let (sn_io, sn_mem_un, sn_mem_pf) = (
                    io_area
                        .map(|ap| {
                            ap.snapshot_aligned(IO_ALIGN).ok_or_else(|| {
                                kerrln!(
                                    "failed to allocate aligned I/O area for device {} when probed by driver {}: aperture has insufficient free space after alignment",
                                    bus_pdev.name(),
                                    self.name(),
                                );
                                SysError::ResourceExhausted
                            })
                        })
                        .transpose()?,
                    mem_area_unpref32
                        .map(|ap| {
                            ap.snapshot_aligned(MEM_ALIGN).ok_or_else(|| {
                                kerrln!(
                                    "failed to allocate aligned non-prefetchable memory area for device {} when probed by driver {}: aperture has insufficient free space after alignment",
                                    bus_pdev.name(),
                                    self.name(),
                                );
                                SysError::ResourceExhausted
                            })
                        })
                        .transpose()?,
                    mem_area_pref32
                        .map(|ap| {
                            ap.snapshot_aligned(MEM_ALIGN).ok_or_else(|| {
                                kerrln!(
                                    "failed to allocate aligned prefetchable memory area for device {} when probed by driver {}: aperture has insufficient free space after alignment",
                                    bus_pdev.name(),
                                    self.name(),
                                );
                                SysError::ResourceExhausted
                            })
                        })
                        .transpose()?,
                );

                // Prefetchable 64-bit memory apertures are not supported.
                // pref64 reuses pref32 and unpref64 is left empty — children
                // must not rely on either.
                let selected_res = AvailableApertures {
                    io_area: io_area.iter().map(|ap| **ap).collect(),
                    mem_area_pref32: mem_area_pref32.iter().map(|ap| **ap).collect(),
                    mem_area_pref64: mem_area_pref32.iter().map(|ap| **ap).collect(),
                    mem_area_unpref32: mem_area_unpref32.iter().map(|ap| **ap).collect(),
                    mem_area_unpref64: vec![],
                };
                bus_dev.for_each_child(|child| {
                    if let Some(driver) = child.driver() {
                        let pdriver = driver
                            .as_pcie_driver()
                            .expect("child of pcie host bridge should be probed with pcie driver");
                        kinfoln!(
                            "allocating resources for pcie bus device {} with driver {}",
                            child.name(),
                            driver.name()
                        );
                        if let Err(e) = pdriver.alloc_resources(child.clone(), &selected_res) {
                            kerrln!(
                                "alloc_resources failed for device {} when probed by driver {}: {:?}",
                                child.name(),
                                driver.name(),
                                e
                            );
                            pdriver.fail(child.as_ref());
                        }
                    }
                });

                // Snapshot apertures after allocation to establish limit addresses.
                let (sn_io_after, sn_mem_un_after, sn_mem_pf_after) = (
                    io_area
                        .map(|ap| {
                            ap.snapshot_aligned(IO_ALIGN).ok_or_else(|| {
                                kerrln!(
                                    "failed to allocate aligned I/O area for device {} when probed by driver {}: aperture has insufficient free space after alignment",
                                    bus_pdev.name(),
                                    self.name(),
                                );
                                SysError::ResourceExhausted
                            })
                        })
                        .transpose()?,
                    mem_area_unpref32
                        .map(|ap| {
                            ap.snapshot_aligned(MEM_ALIGN).ok_or_else(|| {
                                kerrln!(
                                    "failed to allocate aligned non-prefetchable memory area for device {} when probed by driver {}: aperture has insufficient free space after alignment",
                                    bus_pdev.name(),
                                    self.name(),
                                );
                                SysError::ResourceExhausted
                            })
                        })
                        .transpose()?,
                    mem_area_pref32
                        .map(|ap| {
                            ap.snapshot_aligned(MEM_ALIGN).ok_or_else(|| {
                                kerrln!(
                                    "failed to allocate aligned prefetchable memory area for device {} when probed by driver {}: aperture has insufficient free space after alignment",
                                    bus_pdev.name(),
                                    self.name(),
                                );
                                SysError::ResourceExhausted
                            })
                        })
                        .transpose()?,
                );

                let conf = conf
                    .as_type1()
                    .expect("PCIe bus bridge must have a type-1 configuration space");

                // Program I/O base/limit if an I/O aperture was available.
                if let Some(io_base) = sn_io {
                    unsafe {
                        let io_limit = sn_io_after.unwrap();
                        if io_limit > io_base {
                            conf.set_io_base(io_base.try_into().map_err(|_e| {
                                kerrln!("failed to set I/O base for device {} when probed by driver {}: aligned I/O base address {:#x} exceeds the max value {:#x}",
                                    bus_pdev.name(),
                                    self.name(),
                                    io_base,
                                    u32::MAX as u64
                                );
                                SysError::InvalidArgument
                            })?);
                            conf.set_io_limit(io_limit.try_into().map_err(|_e| {
                                kerrln!("failed to set I/O limit for device {} when probed by driver {}: aligned I/O limit address {:#x} exceeds the max value {:#x}",
                                    bus_pdev.name(),
                                    self.name(),
                                    io_limit,
                                    u32::MAX as u64
                                );
                                SysError::InvalidArgument
                            })?);
                        }
                    }
                }

                // Program prefetchable memory base/limit.
                if let Some(mem_base) = sn_mem_pf {
                    unsafe {
                        let mem_limit = sn_mem_pf_after.unwrap();
                        if mem_limit > mem_base {
                            conf.set_prefetchable_mem_base(mem_base);
                            conf.set_prefetchable_mem_limit(mem_limit);
                        }
                    }
                }

                // Program non-prefetchable memory base/limit.
                if let Some(mem_base) = sn_mem_un {
                    unsafe {
                        let mem_limit = sn_mem_un_after.unwrap();
                        if mem_limit > mem_base {
                            conf.set_mem_base(mem_base.try_into().map_err(|_e| {
                                kerrln!("failed to set not-prefetchable memory base for device {} when probed by driver {}: aligned memory base address {:#x} exceeds the max value {:#x}",
                                    bus_pdev.name(),
                                    self.name(),
                                    mem_base,
                                    u32::MAX as u64
                                );
                                SysError::InvalidArgument
                            })?);
                            conf.set_mem_limit(mem_limit.try_into().map_err(|_e| {
                                kerrln!("failed to set not-prefetchable memory limit for device {} when probed by driver {}: aligned memory limit address {:#x} exceeds the max value {:#x}",
                                    bus_pdev.name(),
                                    self.name(),
                                    mem_limit,
                                    u32::MAX as u64
                                );
                                SysError::InvalidArgument
                            })?);
                        }
                    }
                }
            },
            _ => {
                return Err(SysError::DriverIncompatible);
            },
        }
        Ok(())
    }

    /// Propagate post-initialization to all children.
    fn postinit(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        let pdev = device
            .as_pcie_device()
            .expect("pcie driver should only be initialized with pcie device");
        (cast_dev(pdev) as &dyn Device).for_each_child(|child| {
            if let Some(driver) = child.driver() {
                let pdriver = driver
                    .as_pcie_driver()
                    .expect("child of pcie host bridge should be probed with pcie driver");
                pdriver.postinit(child.clone());
            }
        });
        Ok(())
    }

    /// Tear down children in reverse order, then detach this driver.
    fn fail(&self, device: &dyn Device) {
        let mut children = vec![];
        let pdev = device
            .as_pcie_device()
            .expect("pcie driver should only be initialized with pcie device");
        (cast_dev(pdev) as &dyn Device).for_each_child(|child| children.push(child.clone()));

        // Roll back in reverse initialization order.
        children.iter().rev().for_each(|child| {
            if let Some(driver) = child.driver() {
                let pdriver = driver
                    .as_pcie_driver()
                    .expect("child of pcie host bridge should be probed with pcie driver");
                pdriver.fail(child.as_ref());
            }
        });

        device.set_driver(None);
    }
}

/// Create and register the [`BridgeDriver`] singleton.
#[initcall(driver)]
fn init_host_driver() {
    let kobj_base = KObjectBase::new(KObjIdent::try_from("pcie-bridge-driver").unwrap());
    let drv_base = DriverBase::new();
    let driver = Arc::new(BridgeDriver {
        kobj_base,
        drv_base,
    });
    pcie::register_driver(driver);
}

/// Return the bus-side of a bridge device (for iterating children).
fn cast_dev(device: &PcieDevice) -> &PcieDevice {
    match device.dev_info() {
        PcieDeviceType::Bus { .. } => device,
        PcieDeviceType::Endpoint {
            sub_bus: Some(sub_bus),
            ..
        } => sub_bus.as_ref(),
        _ => panic!("invalid device type for bridge driver"),
    }
}

/// Enumerate all PCI functions on the given bus, create child devices, and
/// preinitialize them with matching drivers.
///
/// Type 0 functions become endpoints; Type 1 functions create sub-buses
/// (recursive). Unsupported header layouts are warned and skipped.
fn enum_pcie_bus(bus_dev: &Arc<PcieDevice>, current_bus_id: &BusNum) {
    let domain = bus_dev.domain();
    let ecam = domain.ecam();
    let bus_conf = ecam.get_bus(*current_bus_id);
    kinfoln!("enumerating pcie devices on {:?}", bus_conf.num());

    for dev_id in (DevNumRangeInclusive {
        start: DevNum::MIN,
        end: DevNum::MAX,
    }
    .into_iter())
    {
        let dev_conf = bus_conf.get_device(DevNum::try_from(dev_id).unwrap());
        dev_conf
            .iter_functions(|func_num, func_conf| -> Result<(), SysError> {
                if func_conf.class_code() == CLASSCODE_HOST_BRIDGE {
                    // Skip host bridge functions behind the root complex's primary bus; they are
                    // not real devices.
                    return Ok(());
                }
                let identifier = PciFunctionIdentifier {
                    bus: bus_conf.num(),
                    dev: dev_id,
                    func: func_num,
                };
                let header = func_conf.header_type();
                match header.layout() {
                    Err(e) => {
                        kwarningln!(
                            "unsupported header layout of device {:?} : {:?}",
                            identifier,
                            e
                        );
                        Ok(())
                    },
                    Ok(PciHeaderLayout::Type0) => {
                        create_and_preinit_endpoint(
                            bus_dev,
                            identifier,
                            func_conf.as_type0().unwrap(),
                        );
                        Ok(())
                    },
                    Ok(PciHeaderLayout::Type1) => {
                        if let Err(e) = create_and_preinit_sub_bus(
                            bus_dev,
                            identifier,
                            func_conf.as_type1().unwrap(),
                        ) {
                            kerrln!(
                                "PCIe bus driver: failed to init pcie bus at {:?}: {:?}",
                                identifier,
                                e
                            );
                        }
                        Ok(())
                    },
                }
            })
            .expect("PCIe function iteration should ever return errors");
    }
}

/// Create an endpoint device, look up its interrupt info, and preinitialize it.
fn create_and_preinit_endpoint(
    parent_dev: &Arc<PcieDevice>,
    identifier: PciFunctionIdentifier,
    func_conf: Type0FuncConf,
) {
    let domain = parent_dev.domain();

    let intr_pin = func_conf.general().intr_pin();
    let intr_info = domain
        .resources()
        .find_intr_info(PcieIntrKey {
            func_addr: identifier,
            intr_pin,
        })
        .cloned();

    let device = PcieDevice::new_endpoint(
        KObjIdent::try_from_fmt(format_args!("{}:{}", domain.id(), identifier)).unwrap(),
        domain.clone(),
        identifier,
        intr_info,
        None,
    );

    device.set_parent(Some(parent_dev.clone()));
    let device = Arc::new(device);
    parent_dev.register_and_preinit_device(device);
}

/// Create a child bus (and its bridge endpoint) for a Type-1 function, then
/// preinitialize both.
///
/// The subordinate bus number is set to [`BusNum::MAX`] before the child bus is
/// created so downstream enumeration sees the widest possible range; it is
/// tightened to the domain's current bus number after creation.
fn create_and_preinit_sub_bus(
    parent_dev: &Arc<PcieDevice>,
    identifier: PciFunctionIdentifier,
    conf: Type1FuncConf,
) -> Result<(), SysError> {
    let domain = parent_dev.domain();

    let PciFunctionIdentifier {
        bus,
        dev,
        func: _func,
    } = identifier;

    let new_bus = domain.resources().alloc_bus_num()?;
    let new_bus_u8: u8 = new_bus.into();

    let resources = domain.resources();

    // Pre-set bus number registers with widest possible subordinate range.
    unsafe {
        conf.set_primary_bus_num(bus);
        conf.set_secondary_bus_num(new_bus);
        conf.set_subordinate_bus_num(BusNum::MAX);
    }

    // Clear command register; the sub-bus driver configures it.
    conf.general().write_command(PciCommands::empty());

    let device_bus = PcieDevice::new_bus(
        KObjIdent::try_from_fmt(format_args!("pci{}:{}", domain.id(), new_bus)).unwrap(),
        domain.clone(),
        new_bus,
    );
    let device_bus = Arc::new(device_bus);

    let device_endpoint = PcieDevice::new_endpoint(
        KObjIdent::try_from_fmt(format_args!("{}:{}", domain.id(), identifier)).unwrap(),
        domain.clone(),
        identifier,
        None,
        Some(device_bus.clone()),
    );

    let device_endpoint = Arc::new(device_endpoint);
    device_endpoint.set_parent(Some(parent_dev.clone()));
    parent_dev.register_and_preinit_device(device_endpoint);

    device_bus.set_parent(Some(ROOT.clone()));
    ROOT.add_child(device_bus);

    // Tighten subordinate to the highest bus number actually allocated.
    unsafe {
        conf.set_subordinate_bus_num(resources.current_bus_num());
    }
    Ok(())
}
