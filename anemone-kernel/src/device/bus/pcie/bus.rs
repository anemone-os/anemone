use kernel_macros::KObject;

use crate::{
    device::{
        Device,
        bus::{BusType, BusTypeBase, pcie::domain::AvailableApertures},
        kobject::{KObjIdent, KObjectBase, KObjectOps},
    },
    prelude::*,
};

/// PCIe bus type: manages the collection of registered PCIe devices and
/// drivers and orchestrates the probe sequence (preinit → alloc_resources →
/// postinit → probe).
#[derive(Debug, KObject)]
pub struct PcieBusType {
    #[kobject]
    kobj_base: KObjectBase,
    /// Underlying bus-type bookkeeping (device/driver lists).
    busty_base: BusTypeBase,
}

impl PcieBusType {
    /// Create a new PCIe bus type with the given sysfs name.
    pub fn new(name: KObjIdent) -> Self {
        Self {
            kobj_base: KObjectBase::new(name),
            busty_base: BusTypeBase::new(),
        }
    }
}

impl KObjectOps for PcieBusType {}

impl BusType for PcieBusType {
    fn base(&self) -> &BusTypeBase {
        &self.busty_base
    }

    /// Match a device to a driver by vendor/device ID first, then by class
    /// code.
    fn matches(&self, device: &dyn Device, driver: &dyn Driver) -> bool {
        let pdev = device
            .as_pcie_device()
            .expect("device on PCIe bus is not a PCIe device");
        let pdrv = driver
            .as_pcie_driver()
            .expect("driver on PCIe bus is not a PCIe driver");

        let vendor_device_id = pdev.vendor_device_id();
        if let Some(vendor_device_id) = vendor_device_id
            && pdrv
                .vendor_device_table()
                .iter()
                .any(|&m| vendor_device_id == m)
        {
            return true;
        }
        let class_code = pdev.class_code();
        pdrv.class_code_table().iter().any(|&m| class_code == m)
    }

    /// Register a host bridge device and probe matching drivers.
    fn register_device(&self, device: Arc<dyn Device>) {
        for driver in BusType::base(self).drivers.read_irqsave().iter() {
            if self.matches(device.as_ref(), driver.as_ref()) {
                // TODO: probe defer
                kinfoln!(
                    "initializing pcie bus device {} with driver {}",
                    device.name(),
                    driver.name()
                );
                let pdriver = driver
                    .as_pcie_driver()
                    .expect("only pcie drivers should be registered to pcie bus");
                if let Err(e) = pdriver.preinit(device.clone()) {
                    kerrln!(
                        "preinit failed for device {} when probed by driver {}: {:?}",
                        device.name(),
                        pdriver.name(),
                        e
                    );
                    pdriver.fail(device.as_ref());
                    return;
                }

                let pdev = device
                    .as_pcie_device()
                    .expect("pcie driver should only be probed with pcie device");

                kinfoln!(
                    "allocating resources for pcie bus device {} with driver {}",
                    device.name(),
                    driver.name()
                );
                if let Err(e) = pdriver.alloc_resources(
                    device.clone(),
                    &AvailableApertures::pick_all_from(pdev.domain().resources().aperture()),
                ) {
                    kerrln!(
                        "alloc_resources failed for device {} when probed by driver {}: {:?}",
                        device.name(),
                        pdriver.name(),
                        e
                    );
                    pdriver.fail(device.as_ref());
                    return;
                }

                kinfoln!(
                    "finalizing initialization for pcie bus device {} with driver {}",
                    device.name(),
                    driver.name()
                );
                if let Err(e) = pdriver.postinit(device.clone()) {
                    kerrln!(
                        "postinit failed for device {} when probed by driver {}: {:?}",
                        device.name(),
                        pdriver.name(),
                        e
                    );
                    pdriver.fail(device.as_ref());
                    return;
                }

                kinfoln!(
                    "probing pcie bus device {} with driver {}",
                    device.name(),
                    pdriver.name()
                );
                match pdriver.probe(device.clone()) {
                    Ok(()) => {
                        device.set_driver(Some(driver.clone()));
                        pdriver.attach_device(device.clone());
                    },
                    Err(e) => {
                        kerrln!(
                            "failed to probe device {} with driver {}: {:?}",
                            device.name(),
                            pdriver.name(),
                            e
                        );
                        pdriver.fail(device.as_ref());
                    },
                }
                break;
            }
        }
        BusType::base(self)
            .devices
            .write_irqsave()
            .add_kobject(device);
    }
}
