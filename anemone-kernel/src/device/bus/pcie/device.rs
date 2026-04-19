use core::fmt::Debug;

use kernel_macros::{Device, KObject};

use crate::{
    device::{
        DeviceBase, DeviceOps,
        bus::{
            BusType,
            pcie::{
                HOST_BRIDGE_CLASSCODE, PCIE_BUS_TYPE,
                ecam::{BusNum, ClassCode, DevNum, EcamConf, FuncNum, PcieDeviceConf},
            },
        },
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
    },
    prelude::*,
};

/// PCIe device, which can be either a bus or an endpoint.
#[derive(Debug, KObject, Device)]
pub struct PcieDevice {
    #[kobject]
    kobj_base: KObjectBase,
    #[device]
    dev_base: DeviceBase,

    /// The PCIe domain this device belongs to.
    domain: Arc<PcieDomain>,

    /// Information about this PCIe device, including its children
    /// and whether it's a bus or an endpoint.
    info: PcieDeviceInfo,
}

#[derive(Debug)]
pub struct PcieDomain {
    /// `domain` is the unique PCIe domain identifier.
    domain: usize,
    /// `ecam` is the ECAM configuration used to access PCIe config space.
    ecam: EcamConf,
    /// `bus_num_alloc` tracks the latest allocated bus number in this domain.
    bus_num_alloc: AtomicU8,
}

impl PcieDomain {
    /// [new] creates a PCIe domain from a domain id and ECAM configuration.
    ///
    /// `domain` is the unique domain identifier.
    /// `ecam` provides config-space addressing information.
    pub fn new(domain: usize, ecam: EcamConf) -> Self {
        Self {
            domain,
            bus_num_alloc: AtomicU8::new(ecam.root_bus_num().into()),
            ecam,
        }
    }

    /// [domain_id] returns the domain identifier.
    pub fn domain_id(&self) -> usize {
        self.domain
    }

    /// [ecam] returns the ECAM configuration bound to this domain.
    pub fn ecam(&self) -> &EcamConf {
        &self.ecam
    }

    /// [alloc_bus_num] allocates the next available bus number in this domain.
    pub fn alloc_bus_num(&self) -> Result<BusNum, SysError> {
        let bus_num_u8 = self.bus_num_alloc.load(Ordering::SeqCst);
        let next = bus_num_u8.checked_add(1).ok_or_else(|| {
            kerrln!("Error allocating bus number: the bus number exceeds 255.");
            SysError::InvalidArgument
        })?;
        let new_bus_num = BusNum::try_from(next).map_err(|e| {
            kerrln!(
                "Error allocating bus number: the bus number '{}' exceeds the max value '{:?}'.",
                next,
                self.ecam.max_bus_num()
            );
            e
        })?;
        self.bus_num_alloc
            .store(new_bus_num.into(), Ordering::SeqCst);
        Ok(new_bus_num)
    }

    /// [bus_num] returns the current allocated bus number marker.
    pub fn bus_num(&self) -> BusNum {
        BusNum::try_from(self.bus_num_alloc.load(Ordering::SeqCst)).unwrap()
    }
}

#[derive(Debug)]
pub enum PcieDeviceInfo {
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
    /// [dev_conf] returns the PCIe configuration accessor when available.
    pub fn dev_conf(&self) -> Option<&PcieDeviceConf> {
        match &self.info {
            PcieDeviceInfo::Endpoint { conf, .. } => Some(conf),
            PcieDeviceInfo::Bus { conf, .. } => Some(conf),
            PcieDeviceInfo::HostBridge { .. } => None,
        }
    }

    /// [domain] returns the PCIe domain this device belongs to.
    pub fn domain(&self) -> &Arc<PcieDomain> {
        &self.domain
    }

    /// [dev_info] returns detailed PCIe topology metadata for this device.
    pub fn dev_info(&self) -> &PcieDeviceInfo {
        &self.info
    }

    /// [bus_num] returns the bus number for bus/endpoint devices.
    pub fn bus_num(&self) -> Option<BusNum> {
        match self.info {
            PcieDeviceInfo::HostBridge { .. } => None,
            PcieDeviceInfo::Bus { bus, .. } => Some(bus),
            PcieDeviceInfo::Endpoint { bus, .. } => Some(bus),
        }
    }

    /// [dev_num] returns the device number for bus/endpoint devices.
    pub fn dev_num(&self) -> Option<DevNum> {
        match self.info {
            PcieDeviceInfo::HostBridge { .. } => None,
            PcieDeviceInfo::Bus { dev, .. } => Some(dev),
            PcieDeviceInfo::Endpoint { dev, .. } => Some(dev),
        }
    }

    /// [class_code] reads the class code used for driver matching.
    pub fn class_code(&self) -> ClassCode {
        match &self.info {
            PcieDeviceInfo::HostBridge { .. } => HOST_BRIDGE_CLASSCODE, // Host bridge class code
            PcieDeviceInfo::Bus { conf, .. } => conf.get_function(FuncNum::MIN).class_code(),
            PcieDeviceInfo::Endpoint { conf, .. } => conf.get_function(FuncNum::MIN).class_code(),
        }
    }

    /// [new_endpoint] creates a PCIe endpoint device object.
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
            info: PcieDeviceInfo::Endpoint {
                bus,
                dev,
                conf: domain.ecam.get_bus(bus).get_device(dev),
            },
            domain,
        }
    }

    /// [new_bus] creates a PCIe bus-device object for a bridge function.
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
            info: PcieDeviceInfo::Bus {
                id,
                bus,
                dev,
                conf: domain.ecam.get_bus(bus).get_device(dev),
            },
            domain,
        }
    }

    /// [new_host_bridge] creates a PCIe host-bridge device object.
    ///
    /// `name` is the device kobject name.
    /// `domain` is the owning PCIe domain.
    /// `id` is the root bus number associated with this host bridge.
    pub fn new_host_bridge(name: KObjIdent, domain: Arc<PcieDomain>, id: BusNum) -> Self {
        Self {
            kobj_base: KObjectBase::new(name),
            dev_base: DeviceBase::new(None),
            domain,
            info: PcieDeviceInfo::HostBridge { id: id },
        }
    }

    /// [register_and_add_device] links a child device and probes matching PCIe drivers.
    ///
    /// `device` is the child PCIe device to add under `self`.
    pub fn register_and_add_device(&self, device: Arc<PcieDevice>) {
        if let PcieDeviceInfo::Endpoint { .. } = &self.info {
            panic!("cannot register device to an endpoint");
        }
        self.add_child(device.clone());
        for driver in PCIE_BUS_TYPE.base().drivers.read().iter() {
            if PCIE_BUS_TYPE.matches(device.as_ref(), driver.as_ref()) {
                // TODO: probe defer
                match driver.probe(device.clone()) {
                    Ok(()) => {
                        device.set_driver(Some(driver.clone()));
                        driver.attach_device(device.clone());
                    },
                    Err(e) => {
                        kerrln!(
                            "failed to probe device {} with driver {}: {:?}",
                            device.name(),
                            driver.name(),
                            e
                        );
                    },
                }
                break;
            }
        }
    }
}
