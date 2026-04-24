use core::ops::RangeInclusive;

use crate::{
    device::{
        bus::pcie::{
            self, HOST_BRIDGE_CLASSCODE, PCI2PCI_BRIDGE_CLASSCODE, PciFuncAddr, PcieDevice,
            PcieDeviceType, PcieDriver, PcieIntrKey, PcieMemAreaSnapshot,
            ecam::{BusNum, DevNum, PciClassCode, PciCommands, PciHeaderLayout, Type1FuncConf},
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
    fn class_code_table(&self) -> &'static [PciClassCode] {
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
            PcieDeviceType::Bus { conf, id, .. } => {
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
        let dev_conf = bus.get_device(DevNum::try_from(dev).unwrap());
        dev_conf
            .iter_functions(|func_num, fconf| -> Result<(), SysError> {
                let func_num_u8: u8 = func_num.into();
                let bus_num = bus.num();
                let bus_num_u8: u8 = bus_num.into();
                let func_addr = PciFuncAddr {
                    bus: bus_num,
                    dev: DevNum::try_from(dev).unwrap(),
                    func: func_num,
                };
                let header = fconf.header_type();
                match header.layout() {
                    Err(e) => {
                        kwarningln!(
                            "unsupported header layout of device {:?} : {:?}",
                            func_addr,
                            e
                        );
                        Ok(())
                    },
                    Ok(PciHeaderLayout::Type0) => {
                        let domain = parent_dev.domain();
                        let intr_pin = fconf.intr_pin();
                        let intr_info = domain
                            .resources()
                            .find_intr_info(PcieIntrKey {
                                func_addr,
                                intr_pin,
                            })
                            .cloned();
                        let device = PcieDevice::new_endpoint(
                            KObjIdent::try_from_fmt(format_args!(
                                "{:04x}:{:02x}:{:02x}.{}",
                                domain.id(),
                                bus_num_u8,
                                dev,
                                func_num_u8
                            ))
                            .unwrap(),
                            domain.clone(),
                            func_addr,
                            intr_info,
                        );
                        device.set_parent(Some(parent_dev.clone()));
                        let device = Arc::new(device);
                        parent_dev.register_and_preinit_device(device);
                        Ok(())
                    },
                    Ok(PciHeaderLayout::Type1) => {
                        if let Err(e) =
                            init_pcie_bus(parent_dev, func_addr, fconf.as_type1().unwrap())
                        {
                            kerrln!(
                                "PCIe bus driver: failed to init pcie bus at {:?}: {:?}",
                                func_addr,
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

const MEM_AREA_ALIGN: PcieMemAreaSnapshot = PcieMemAreaSnapshot {
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
    addr: PciFuncAddr,
    conf: Type1FuncConf,
) -> Result<(), SysError> {
    let domain = parent_dev.domain();
    let PciFuncAddr { bus, dev, func } = addr;

    let bus_num_u8: u8 = bus.into();

    let new_bus = domain.resources().alloc_bus_num()?;
    let new_bus_u8: u8 = new_bus.into();

    let dev_num_u8: u8 = dev.into();

    let func_num_u8: u8 = func.into();

    let resources = domain.resources();

    unsafe {
        conf.set_primary_bus_num(bus);
        conf.set_secondary_bus_num(new_bus);
        conf.set_subordinate_bus_num(BusNum::MAX);
    }

    let snapshot_before_init = resources.snapshot_mems(MEM_AREA_ALIGN);

    conf.general().write_command(PciCommands::empty());

    let device = PcieDevice::new_bus(
        KObjIdent::try_from_fmt(format_args!(
            "{:04x}:{:02x}:{:02x}.{}",
            parent_dev.domain().id(),
            bus_num_u8,
            dev_num_u8,
            func_num_u8
        ))
        .unwrap(),
        parent_dev.domain().clone(),
        addr,
        new_bus,
    );

    let device = Arc::new(device);
    device.set_parent(Some(parent_dev.clone()));
    parent_dev.register_and_preinit_device(device);

    unsafe {
        conf.set_subordinate_bus_num(resources.current_bus_num());

        if let Some(snapshot) = resources.snapshot_mems(MEM_AREA_ALIGN)
            && let Some(before) = snapshot_before_init
        {
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
