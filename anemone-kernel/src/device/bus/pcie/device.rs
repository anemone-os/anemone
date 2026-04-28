//! PCIe device objects: host bridge, endpoint, BAR bookkeeping, and device
//! registration/probing within a [`PcieDomain`](super::domain::PcieDomain).

use core::fmt::Debug;

use kernel_macros::{Device, KObject};

use crate::{
    device::{
        DeviceBase, DeviceOps,
        bus::{
            BusType,
            pcie::{
                CLASSCODE_HOST_BRIDGE, PCIE_BUS_TYPE, PciFunctionIdentifier, PcieFwNode,
                domain::{PcieDomain, PcieIntrInfo, PcieMemArea},
                ecam::{BusNum, FuncConf, PciBar, PciClassCode},
            },
        },
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
    },
    mm::remap::IoRemap,
    prelude::*,
};

/// A PCIe device: host bridge or endpoint.
#[derive(Debug, KObject, Device)]
pub struct PcieDevice {
    #[kobject]
    kobj_base: KObjectBase,
    #[device]
    dev_base: DeviceBase,

    /// Owning PCIe domain.
    domain: Arc<PcieDomain>,

    /// PCIe type and topology metadata.
    typed_info: PcieDeviceType,

    /// Allocated BAR regions.
    bar_info: RwLock<Option<Vec<DeviceBarInfo>>>,
}

#[cfg(debug_assertions)]
impl Drop for PcieDevice {
    fn drop(&mut self) {
        if let Some(bar_info) = &self.bar_info.read().as_ref() {
            if bar_info.len() > 0 {
                panic!(
                    "PCIe device {} is being dropped while its BARs are still allocated!",
                    self.name()
                );
            }
        }
    }
}

/// Bookkeeping for one allocated PCIe BAR.
#[derive(Debug)]
pub struct DeviceBarInfo {
    /// BAR index (0–5).
    pub bar_idx: usize,
    /// Raw BAR register value.
    pub bar: PciBar,
    /// Physical memory region assigned to this BAR.
    pub mem_area: PcieMemArea,
    /// MMIO remapping handle.
    pub remap: Arc<IoRemap>,
}

/// Device kind and topology metadata.
///
/// A PCIe-to-PCI bridge is represented by two [`PcieDevice`] instances:
/// one [`Bus`](PcieDeviceType::Bus) registered under the root bus, and one
/// [`Endpoint`](PcieDeviceType::Endpoint) whose
/// [`sub_bus`](PcieDeviceType::Endpoint::sub_bus) points to the `Bus` instance.
#[derive(Debug)]
pub enum PcieDeviceType {
    /// Host bridge, identified by its root bus number.
    Bus { id: BusNum },
    /// Function on a PCIe bus; may also be a bridge with a subordinate bus.
    Endpoint {
        conf: FuncConf,
        addr: PciFunctionIdentifier,
        /// Child bridge bus, if this endpoint is a PCIe-to-PCI bridge.
        sub_bus: Option<Arc<PcieDevice>>,
    },
}

impl KObjectOps for PcieDevice {}

impl DeviceOps for PcieDevice {}

impl PcieDevice {
    /// PCIe configuration space accessor, if this is an endpoint.
    pub fn func_conf(&self) -> Option<&FuncConf> {
        match &self.typed_info {
            PcieDeviceType::Endpoint { conf, .. } => Some(conf),
            PcieDeviceType::Bus { .. } => None,
        }
    }

    /// Owning PCIe domain.
    pub fn domain(&self) -> &Arc<PcieDomain> {
        &self.domain
    }

    /// PCIe type and topology metadata.
    pub fn dev_info(&self) -> &PcieDeviceType {
        &self.typed_info
    }

    /// PCI function address, if this is an endpoint.
    pub fn identifier(&self) -> Option<PciFunctionIdentifier> {
        match &self.typed_info {
            PcieDeviceType::Endpoint { addr, .. } => Some(*addr),
            PcieDeviceType::Bus { .. } => None,
        }
    }

    /// Class code used for driver matching.
    pub fn class_code(&self) -> PciClassCode {
        match &self.typed_info {
            PcieDeviceType::Bus { .. } => CLASSCODE_HOST_BRIDGE,
            PcieDeviceType::Endpoint { conf, .. } => conf.class_code(),
        }
    }

    /// `(vendor_id, device_id)` for endpoints, `None` for a bus.
    pub fn vendor_device_id(&self) -> Option<(u16, u16)> {
        match &self.typed_info {
            PcieDeviceType::Bus { .. } => None,
            PcieDeviceType::Endpoint { conf, .. } => {
                let func = conf;
                Some((func.vendor_id(), func.device_id()))
            },
        }
    }

    /// Iterate over allocated BARs.
    pub fn iter_bar_info<F: Fn(&DeviceBarInfo)>(&self, f: F) {
        if let Some(bar_info) = &self.bar_info.read().as_ref() {
            for info in bar_info.iter() {
                f(info);
            }
        }
    }

    /// Set allocated BAR info. Panics in debug builds if BARs are already set.
    pub fn set_bar_info(&self, bar_info: Vec<DeviceBarInfo>) {
        #[cfg(debug_assertions)]
        {
            if self.bar_info.read().as_ref().map_or(false, |b| b.len() > 0) {
                panic!(
                    "attempting to set BAR info for PCIe device {} while it already has allocated BARs!",
                    self.name()
                );
            }
        }
        *self.bar_info.write() = Some(bar_info);
    }

    /// Create an endpoint device.
    pub fn new_endpoint(
        name: KObjIdent,
        domain: Arc<PcieDomain>,
        addr: PciFunctionIdentifier,
        intr_info: Option<PcieIntrInfo>,
        sub_bus: Option<Arc<PcieDevice>>,
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
                sub_bus,
            },
            domain,
            bar_info: RwLock::new(None),
        }
    }

    /// Create a host-bridge device.
    pub fn new_bus(name: KObjIdent, domain: Arc<PcieDomain>, id: BusNum) -> Self {
        Self {
            kobj_base: KObjectBase::new(name),
            dev_base: DeviceBase::new(Some(Arc::new(PcieFwNode::new(None)))),
            domain,
            typed_info: PcieDeviceType::Bus { id: id },
            bar_info: RwLock::new(None),
        }
    }

    /// Register an endpoint child and run driver preinit. Panics if called on
    /// an endpoint.
    pub fn register_and_preinit_device(&self, device: Arc<PcieDevice>) {
        if let PcieDeviceType::Endpoint { .. } = &self.typed_info {
            panic!("cannot register device to an endpoint");
        }
        self.add_child(device.clone());

        for driver in PCIE_BUS_TYPE.base().drivers.read().iter() {
            if PCIE_BUS_TYPE.matches(device.as_ref(), driver.as_ref()) {
                // TODO: probe defer
                let pcie_driver = driver
                    .as_pcie_driver()
                    .expect("only pcie drivers should be registered to pcie bus");

                if let Err(e) = pcie_driver.preinit(device.clone()) {
                    kerrln!(
                        "preinit failed for device {} when probed by driver {}: {:?}",
                        device.name(),
                        driver.name(),
                        e
                    );
                    pcie_driver.fail(device.as_ref());
                    return;
                }
                device.set_driver(Some(driver.clone()));
                // attach will be called after probe
                break;
            }
        }
    }

    /// Probe all child devices, then recurse into any bridge sub-bus.
    pub fn probe_all_devices(&self) {
        (self as &dyn Device).for_each_child(|child| {
            if let Some(driver) = child.driver() {
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
        if let PcieDeviceType::Endpoint {
            sub_bus: Some(sub_bus),
            ..
        } = &self.typed_info
        {
            sub_bus.probe_all_devices();
        }
    }
}
