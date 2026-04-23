use core::ops::RangeInclusive;

use crate::{
    device::{
        bus::pcie::{
            self, HOST_BRIDGE_CLASSCODE, PCI2PCI_BRIDGE_CLASSCODE, PciMemAreaSnapshot, PcieDevice,
            PcieDeviceType, PcieDriver,
            ecam::{
                BusNum, ClassCode, DevNum, FuncNum, PciCommands, PciHeaderLayout, Type1FuncConf,
            },
        },
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
    },
    prelude::*,
};

#[derive(Debug, KObject, Driver)]
struct BridgeDriver {
    /// `kobj_base` stores the common kobject metadata for this driver instance.
    #[kobject]
    kobj_base: KObjectBase,
    /// `drv_base` stores the common driver metadata and callbacks wiring.
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for BridgeDriver {}

impl DriverOps for BridgeDriver {
    /// [probe] initializes PCIe bridge-like devices and starts child-bus
    /// enumeration.
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        /*kinfoln!("PCIe bridge regs:");
        if let Some(conf) = device.as_pcie_device().unwrap().dev_conf() {
            let conf = conf.get_function(FuncNum::MIN);
            let type1 = conf.as_type1().unwrap();
            kinfoln!("  Primary bus number: {:?}", type1.primary_bus_num());
            kinfoln!("  Secondary bus number: {:?}", type1.secondary_bus_num());
            kinfoln!(
                "  Subordinate bus number: {:?}",
                type1.subordinate_bus_num()
            );
            kinfoln!("  I/O base: {:#x}", type1.io_base());
            kinfoln!("  I/O limit: {:#x}", type1.io_limit());
            kinfoln!("  Memory base: {:#x}", type1.mem_base());
            kinfoln!("  Memory limit: {:#x}", type1.mem_limit());
            kinfoln!(
                "  Prefetchable memory base: {:#x}",
                type1.prefetchable_mem_base()
            );
            kinfoln!(
                "  Prefetchable memory limit: {:#x}",
                type1.prefetchable_mem_limit()
            );
            kinfoln!("  Command: {:?}", conf.command());
        }*/
        let pdev = device
            .as_pcie_device()
            .expect("pcie driver should only be probed with pcie device");
        pdev.probe_all_devices();
        Ok(())
    }

    fn shutdown(&self, device: &dyn Device) {
        //todo!()
    }

    fn as_pcie_driver(&self) -> Option<&dyn PcieDriver> {
        Some(self as &dyn PcieDriver)
    }
}

impl PcieDriver for BridgeDriver {
    fn class_code_table(&self) -> &'static [ClassCode] {
        &[PCI2PCI_BRIDGE_CLASSCODE, HOST_BRIDGE_CLASSCODE] // PCI-to-PCI bridge and host bridge
    }

    fn vendor_device_table(&self) -> &[(u16, u16)] {
        &[]
    }

    fn postinit(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        let pdev = Arc::downcast::<PcieDevice>(device)
            .expect("pcie driver should only be initialized with pcie device");
        match pdev.dev_info() {
            PcieDeviceType::HostBridge { id } => {
                enum_pcie_bus(&pdev, id);
            },
            PcieDeviceType::Bus { conf, id, bus, dev } => {
                enum_pcie_bus(&pdev, id);
            },
            _ => {
                return Err(SysError::DriverIncompatible);
            },
        }
        Ok(())
    }
}

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

/// [enum_pcie_bus] scans all device slots on the given bus and registers
/// discovered children.
///
/// `parent_dev` provides domain and parent-child relationships for discovered
/// devices. `bus_id` identifies which bus to enumerate through ECAM.
fn enum_pcie_bus(parent_dev: &Arc<PcieDevice>, bus_id: &BusNum) {
    let domain = parent_dev.domain();
    let ecam = domain.ecam();
    let bus = ecam.get_bus(*bus_id);
    kinfoln!("enumerating pcie devices on {:?}", bus.num());
    for dev in RangeInclusive::<u8>::new(DevNum::MIN.into(), DevNum::MAX.into()) {
        let dregs = bus.get_device(DevNum::try_from(dev).unwrap());
        let fregs = dregs.get_function(FuncNum::MIN);
        if fregs.exists() {
            /*kinfoln!(
                "Bus #{:?}, Device #{}, Function #{}, Vendor #{:#x}, Type #{:#x}: Status {:?}, Command {:?}, Revision Id {:#x}, Class Code {:?}, Cache Line Size {:#x}, Latency Timer {:#x}, Header Type {:?}, BIST {:#x}",
                bus.num(),
                dev,
                0,
                fregs.vendor_id(),
                fregs.device_id(),
                fregs.status(),
                fregs.command(),
                fregs.revision_id(),
                fregs.class_code(),
                fregs.cache_line_sz(),
                fregs.latency_timer(),
                fregs.header_type(),
                fregs.bist()
            );*/
            match fregs.header_type().layout() {
                Err(e) => {
                    kwarningln!(
                        "unsupported header layout of device #{} at pcie root bus: {:?}",
                        dev,
                        e
                    );
                },
                Ok(PciHeaderLayout::Type0) => {
                    let bus_num = bus.num();
                    let bus_num_u8: u8 = bus_num.into();
                    let domain = parent_dev.domain();
                    let device = PcieDevice::new_endpoint(
                        KObjIdent::try_from_fmt(format_args!(
                            "{:04x}:{:02x}:{:02x}",
                            domain.id(),
                            bus_num_u8,
                            dev,
                        ))
                        .unwrap(),
                        domain.clone(),
                        bus_num,
                        DevNum::try_from(dev).unwrap(),
                    );
                    device.set_parent(Some(parent_dev.clone()));
                    let device = Arc::new(device);
                    parent_dev.register_and_preinit_device(device);
                },
                Ok(PciHeaderLayout::Type1) => {
                    if let Err(e) = init_pcie_bus(
                        parent_dev,
                        bus.num(),
                        DevNum::try_from(dev).unwrap(),
                        fregs.as_type1().unwrap(),
                    ) {
                        kwarningln!(
                            "failed to init pcie bus at bus #{:?}, device #{}",
                            bus.num(),
                            dev
                        );
                    }
                },
            }
        }
    }
}

const MEM_AREA_ALIGN: PciMemAreaSnapshot = PciMemAreaSnapshot {
    io_area: Some(4096),
    mem_area_pref: Some(0x100000),   // 1 MiB
    mem_area_unpref: Some(0x100000), // 1 MiB
};

/// [init_pcie_bus] configures a downstream PCIe bridge and creates a child bus
/// device.
///
/// `parent_dev` is the upstream PCIe device that owns this bridge.
/// `bus_num` and `dev_num` identify the bridge location on the parent bus.
/// `conf` is the Type-1 configuration accessor used to program bus numbers.
fn init_pcie_bus(
    parent_dev: &Arc<PcieDevice>,
    bus_num: BusNum,
    dev_num: DevNum,
    conf: Type1FuncConf,
) -> Result<(), SysError> {
    let domain = parent_dev.domain();
    let bus_num_u8: u8 = bus_num.into();
    let new_bus = domain.alloc_bus_num()?;
    let new_bus_u8: u8 = new_bus.into();
    let dev_num_u8: u8 = dev_num.into();
    unsafe {
        conf.set_primary_bus_num(bus_num);
        conf.set_secondary_bus_num(new_bus);
        conf.set_subordinate_bus_num(BusNum::MAX);
    }
    let snapshot_before = domain.snapshot(MEM_AREA_ALIGN);
    conf.general().write_command(PciCommands::empty());
    let device = PcieDevice::new_bus(
        KObjIdent::try_from_fmt(format_args!(
            "{:04x}:{:02x}:{:02x}",
            parent_dev.domain().id(),
            bus_num_u8,
            dev_num_u8
        ))
        .unwrap(),
        parent_dev.domain().clone(),
        bus_num,
        dev_num,
        new_bus,
    );
    let device = Arc::new(device);
    device.set_parent(Some(parent_dev.clone()));
    parent_dev.register_and_preinit_device(device);
    unsafe {
        conf.set_subordinate_bus_num(domain.current_max_bus_num());
        if let Some(snapshot) = domain.snapshot(MEM_AREA_ALIGN)
            && let Some(before) = snapshot_before
        {
            /*kinfoln!(
                "snapshot of memory areas after initializing bus #{:?} at device #{:?}: {:?}",
                bus_num,
                dev_num,
                snapshot
            );*/
            if snapshot.io_area > before.io_area {
                conf.set_io_base(before.io_area.unwrap_or(0) as u32);
                conf.set_io_limit(snapshot.io_area.and_then(|x| Some(x - 1)).unwrap_or(0) as u32);
            }

            if snapshot.mem_area_unpref > before.mem_area_unpref {
                conf.set_mem_base(before.mem_area_unpref.unwrap_or(0) as u32);
                conf.set_mem_limit(
                    snapshot
                        .mem_area_unpref
                        .and_then(|x| Some(x - 1))
                        .unwrap_or(0) as u32,
                );
            }

            if snapshot.mem_area_pref > before.mem_area_pref {
                conf.set_prefetchable_mem_base(before.mem_area_pref.unwrap_or(0) as u64);
                conf.set_prefetchable_mem_limit(
                    snapshot
                        .mem_area_pref
                        .and_then(|x| Some(x - 1))
                        .unwrap_or(0) as u64,
                );
            }
        }
    }
    conf.general()
        .write_command(PciCommands::MEM_SPACE | PciCommands::IO_SPACE | PciCommands::BUS_MASTER);
    Ok(())
}
