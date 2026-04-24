use core::fmt::Debug;

use kernel_macros::{Device, KObject};

use crate::{
    device::{
        DeviceBase, DeviceOps,
        bus::{
            BusType,
            pcie::{
                HOST_BRIDGE_CLASSCODE, PCIE_BUS_TYPE, PciFuncAddr, PcieDomain, PcieFwNode,
                PcieIntrInfo,
                bus::preinit_device,
                ecam::{BusNum, FuncConf, PciClassCode},
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
pub enum PcieDeviceType {
    /// Host bridge info.
    HostBridge {
        /// `id` is the root bus number represented by this host bridge.
        id: BusNum,
    },
    /// Bus device, which can have child devices.
    Bus {
        /// `conf` is the device configuration accessor for this bus function.
        conf: FuncConf,
        /// `id` is the secondary bus number exposed by this bridge.
        id: BusNum,
        /// `addr` is the PCI function address for this bus device.
        addr: PciFuncAddr,
    },
    /// Endpoint device, which has no child devices.
    Endpoint {
        /// `conf` is the endpoint's configuration accessor.
        conf: FuncConf,
        /// `addr` is the PCI function address for this endpoint.
        addr: PciFuncAddr,
    },
}

impl KObjectOps for PcieDevice {}

impl DeviceOps for PcieDevice {}

impl PcieDevice {
    /// Return the PCIe configuration accessor when available.
    pub fn func_conf(&self) -> Option<&FuncConf> {
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

    /// Return the PCI function address for this device when applicable.
    pub fn func_addr(&self) -> Option<PciFuncAddr> {
        match &self.typed_info {
            PcieDeviceType::Endpoint { addr, .. } => Some(*addr),
            PcieDeviceType::Bus { addr, .. } => Some(*addr),
            PcieDeviceType::HostBridge { .. } => None,
        }
    }

    /// Read the class code used for driver matching.
    pub fn class_code(&self) -> PciClassCode {
        match &self.typed_info {
            PcieDeviceType::HostBridge { .. } => HOST_BRIDGE_CLASSCODE, // Host bridge class code
            PcieDeviceType::Bus { conf, .. } => conf.class_code(),
            PcieDeviceType::Endpoint { conf, .. } => conf.class_code(),
        }
    }

    pub fn vendor_device_id(&self) -> Option<(u16, u16)> {
        match &self.typed_info {
            PcieDeviceType::HostBridge { .. } => None,
            PcieDeviceType::Bus { conf, .. } => {
                let func = conf;
                Some((func.vendor_id(), func.device_id()))
            },
            PcieDeviceType::Endpoint { conf, .. } => {
                let func = conf;
                Some((func.vendor_id(), func.device_id()))
            },
        }
    }

    /// Create a PCIe endpoint device object.
    ///
    /// `name` is the device kobject name.
    /// `domain` is the owning PCIe domain.
    /// `addr` is the PCI function address for this endpoint.
    pub fn new_endpoint(
        name: KObjIdent,
        domain: Arc<PcieDomain>,
        addr: PciFuncAddr,
        intr_info: Option<PcieIntrInfo>,
    ) -> Self {
        Self {
            kobj_base: KObjectBase::new(name),
            dev_base: DeviceBase::new(Some(Arc::new(PcieFwNode::new(intr_info)))),
            typed_info: PcieDeviceType::Endpoint {
                addr: addr,
                conf: domain
                    .ecam()
                    .get_bus(addr.bus)
                    .get_device(addr.dev)
                    .get_function(addr.func),
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
        addr: PciFuncAddr,
        id: BusNum,
    ) -> Self {
        Self {
            kobj_base: KObjectBase::new(name),
            dev_base: DeviceBase::new(Some(Arc::new(PcieFwNode::new(None)))),
            typed_info: PcieDeviceType::Bus {
                id,
                addr: addr,
                conf: domain
                    .ecam()
                    .get_bus(addr.bus)
                    .get_device(addr.dev)
                    .get_function(addr.func),
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
            dev_base: DeviceBase::new(Some(Arc::new(PcieFwNode::new(None)))),
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
                if let Err(e) = preinit_device(device.as_ref()) {
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
