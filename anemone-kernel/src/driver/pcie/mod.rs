use kernel_macros::{Driver, KObject};

use crate::{
    device::{
        bus::platform::{self, PlatformDriver},
        discovery::fwnode::FwNode,
        kobject::{KObjIdent, KObjectBase, KObjectOps},
        resource::Resource,
    },
    driver::{
        DriverBase, DriverOps,
        pcie::ecam::{BusNum, DevNum, EcamConf, FuncNum, PciHeaderLayout, PcieBus, Type1FuncConf},
    },
    mm::remap::ioremap,
    prelude::*,
};

pub mod ecam;

#[derive(Debug, KObject, Driver)]
struct PcieDriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for PcieDriver {}

impl DriverOps for PcieDriver {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        let pdev = device
            .as_platform_device()
            .expect("platform driver should only be probed with platform device");

        let of_node = pdev
            .fwnode()
            .ok_or(SysError::FwNodeLookupFailed)?
            .as_of_node()
            .ok_or(SysError::DriverIncompatible)?;

        let (base, len) = pdev
            .resources()
            .iter()
            .find_map(|resource| match resource {
                Resource::Mmio { base, len } => Some((*base, *len)),
            })
            .ok_or(SysError::MissingResource)?;

        let (root_bus_num, max_bus_num) = of_node
            .prop_read_raw("bus-range")
            .and_then(|arr| {
                kprintln!("bus-range: {:?}", arr);
                if arr.len() != 8 {
                    return Some(Err(SysError::InvalidArgument));
                }
                let root = u32::from_be_bytes(arr[0..4].try_into().unwrap());
                let max = u32::from_be_bytes(arr[4..8].try_into().unwrap());
                if root >= 256 || max >= 256 {
                    return Some(Err(SysError::InvalidArgument));
                }
                Some(Ok((root as u8, max as u8)))
            })
            .unwrap_or(Ok((0, 255)))?;
        let root_bus_num = BusNum::try_from(root_bus_num).unwrap();
        let max_bus_num = BusNum::try_from(max_bus_num).unwrap();

        let remap = unsafe { ioremap(base, len) }?;

        let regs = unsafe { EcamConf::new(&remap, root_bus_num, max_bus_num)? };
        let root_bus = regs.root_bus();
        let mut bus_num_alloc = root_bus_num;
        enum_pcie_bus(&mut bus_num_alloc, &regs, &root_bus);
        loop {}
        Ok(())
    }

    fn as_platform_driver(&self) -> Option<&dyn PlatformDriver> {
        Some(self)
    }

    fn shutdown(&self, device: &dyn Device) {
        // todo
    }
}

fn enum_pcie_bus(bus_num_alloc: &mut BusNum, regs: &EcamConf, bus: &PcieBus) {
    kinfoln!("enumerating devices on bus #{:?}", bus.num());
    for dev in DevNum::MIN..=DevNum::MAX {
        let fregs = bus.get_function(dev, FuncNum::MIN);
        if fregs.exists() {
            kinfoln!(
                "Bus #{:?}, Device #{}, Function #{}, Vendor #{:#x}, Type #{:#x}: Status {:?}, Command {:?}, Revision Id {:#x}, Class Code {:#x}, Cache Line Size {:#x}, Latency Timer {:#x}, Header Type {:?}, BIST {:#x}",
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
            );
            match fregs.header_type().layout() {
                Err(e) => {
                    kwarningln!(
                        "unsupported header layout of device #{} at pcie root bus: {:?}",
                        dev,
                        e
                    );
                },
                Ok(PciHeaderLayout::Type0) => {},
                Ok(PciHeaderLayout::Type1) => {
                    if let Err(e) = init_pcie_bus(bus_num_alloc, regs, fregs.as_type1().unwrap()) {
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

fn alloc_next(current: BusNum, regs: &EcamConf) -> Result<BusNum, SysError> {
    let bus_num_u8: u8 = current.into();
    let next = bus_num_u8.checked_add(1).ok_or_else(|| {
        kerrln!("Error initializing pcie-bus: the bus number exceeds 255.");
        SysError::InvalidArgument
    })?;
    let new_bus_num = BusNum::try_from(next).map_err(|e| {
        kerrln!(
            "Error initializing pcie-bus: the bus number '{}' exceeds the max value '{:?}'.",
            next,
            regs.max_bus_num()
        );
        e
    });
    new_bus_num
}

fn init_pcie_bus(
    bus_num_alloc: &mut BusNum,
    regs: &EcamConf,
    conf: Type1FuncConf,
) -> Result<(), SysError> {
    let bus_num_u8: u8 = (*bus_num_alloc).into();
    *bus_num_alloc = alloc_next(*bus_num_alloc, regs)?;
    unsafe {
        conf.set_secondary_bus_num(*bus_num_alloc);
    }
    let bus = regs.get_bus(*bus_num_alloc);
    enum_pcie_bus(bus_num_alloc, regs, &bus);
    unsafe {
        conf.set_subordinate_bus_num(*bus_num_alloc);
    }
    Ok(())
}

impl PlatformDriver for PcieDriver {
    fn match_table(&self) -> &[&str] {
        &["pci-host-ecam-generic"]
    }
}

#[initcall(driver)]
fn init() {
    let kobj_base = KObjectBase::new(KObjIdent::try_from("pci-host-ecam-generic").unwrap());
    let drv_base = DriverBase::new();
    let driver = Arc::new(PcieDriver {
        kobj_base,
        drv_base,
    });

    platform::register_driver(driver);
}
