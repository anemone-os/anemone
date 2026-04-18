use kernel_macros::{Driver, KObject};

use crate::{
    device::{
        bus::platform::{self, PlatformDriver},
        discovery::fwnode::FwNode,
        kobject::{KObjIdent, KObjectBase, KObjectOps},
        resource::Resource,
    },
    driver::{DriverBase, DriverOps, pci::ecam::EcamConf},
    mm::remap::ioremap,
    prelude::*,
};

pub mod ecam;

#[derive(Debug, KObject, Driver)]
struct PciDriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for PciDriver {}

impl DriverOps for PciDriver {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), DevError> {
        let pdev = device
            .as_platform_device()
            .expect("platform driver should only be probed with platform device");

        let of_node = pdev
            .fwnode()
            .ok_or(DevError::MissingFwNode)?
            .as_of_node()
            .ok_or(DevError::DriverIncompatible)?;

        let (base, len) = pdev
            .resources()
            .iter()
            .find_map(|resource| match resource {
                Resource::Mmio { base, len } => Some((*base, *len)),
            })
            .ok_or(DevError::MissingResource)?;

        let (root_bus, max_bus) = of_node
            .prop_read_raw("bus-range")
            .and_then(|arr| {
                kprintln!("bus-range: {:?}", arr);
                if arr.len() != 8 {
                    return Some(Err(DevError::InvalidArgument));
                }
                let root = u32::from_be_bytes(arr[0..4].try_into().unwrap());
                let max = u32::from_be_bytes(arr[4..8].try_into().unwrap());
                if root >= 256 || max >= 256 {
                    return Some(Err(DevError::InvalidArgument));
                }
                Some(Ok((root as u8, max as u8)))
            })
            .unwrap_or(Ok((0, 255)))?;

        let remap = unsafe { ioremap(base, len) }.map_err(DevError::IoRemapFailed)?;

        let regs = unsafe { EcamConf::new(&remap, root_bus, max_bus)? };
        let root_bus = regs.root_bus();
        for dev in 0..32 {
            let fregs = root_bus.get_function(dev, 0);
            if fregs.exists() {
                kprintln!(
                    "Bus #{}, Device #{}, Function #{}, Vendor #{:#x}, Type #{:#x}",
                    0,
                    dev,
                    0,
                    fregs.vendor_id(),
                    fregs.device_id()
                );
            }
        }
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

impl PlatformDriver for PciDriver {
    fn match_table(&self) -> &[&str] {
        &["pci-host-ecam-generic"]
    }
}

#[initcall(driver)]
fn init() {
    let kobj_base = KObjectBase::new(KObjIdent::try_from("pci-host-ecam-generic").unwrap());
    let drv_base = DriverBase::new();
    let driver = Arc::new(PciDriver {
        kobj_base,
        drv_base,
    });

    platform::register_driver(driver);
}
