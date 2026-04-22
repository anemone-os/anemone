use core::u64;

use kernel_macros::KObject;

use crate::{
    device::{
        Device,
        bus::{
            BusType, BusTypeBase,
            pcie::{
                AvailPciMemArea, PciMemArea, PcieDevice, PcieDomain,
                ecam::{BAR, FuncNum, GeneralFuncConf, MemBARType, PciCommands},
                remap::add_remap_region,
            },
        },
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
    },
    mm::remap::{IoRemap, ioremap},
    prelude::*,
};

#[derive(Debug, KObject)]
/// Represent the PCIe bus type and its base objects.
pub struct PcieBusType {
    #[kobject]
    kobj_base: KObjectBase,
    busty_base: BusTypeBase,
}

impl PcieBusType {
    pub fn new(name: KObjIdent) -> Self {
        Self {
            kobj_base: KObjectBase::new(name),
            busty_base: BusTypeBase::new(),
        }
    }
}

impl KObjectOps for PcieBusType {}

/// Enumerate BARs exposed by `func` on `dev` and return their indices and
/// descriptors.
fn list_bars(dev: &PcieDevice, func: &GeneralFuncConf) -> Result<Vec<(usize, BAR)>, SysError> {
    let layout = func.header_type().layout().map_err(|e| {
        kwarningln!(
            "PCIe device preinit failed: error reading header type for device {}: {:?}",
            dev.name(),
            e
        );
        e
    })?;
    let bar_count = func.bar_count().map_err(|e| {
        kwarningln!(
            "PCIe device preinit failed: error reading BAR count for device {}: {:?}",
            dev.name(),
            e
        );
        e
    })?;
    let mut bar_idx = 0;
    let mut res = vec![];
    while bar_idx < bar_count {
        let bar = func.read_bar(bar_idx).map_err(|e| {
            kwarningln!(
                "PCIe device preinit failed: error reading BAR #{} for device {}: {:?}",
                bar_idx,
                dev.name(),
                e
            );
            e
        })?;
        match bar {
            BAR::Memory {
                mtype: MemBARType::W64,
                ..
            } => {
                res.push((bar_idx, bar));
                bar_idx += 2;
            },
            _ => {
                res.push((bar_idx, bar));
                bar_idx += 1;
            },
        }
    }
    Ok(res)
}

/// Allocate a compatible memory area for `bar` of `size` from `domain` and map
/// it.
///
/// Return the matched aperture, allocated `PciMemArea`, and the `IoRemap` on
/// success.
fn alloc_mem_for_bar(
    domain: &PcieDomain,
    bar: BAR,
    size: u64,
) -> Result<(&AvailPciMemArea, PciMemArea, IoRemap), SysError> {
    let (areas, area) = domain
        .alloc_mem_for_bar(bar, size)
        .ok_or(SysError::ResourceExhausted)?;
    let remap = match unsafe { ioremap(area.phys_addr(), area.size() as usize) } {
        Ok(remap) => remap,
        Err(e) => {
            unsafe {
                areas.free(area);
            }
            return Err(e);
        },
    };
    Ok((areas, area, remap))
}

/// Pre-initialize a function: disable legacy command bits, size-probe BARs,
/// allocate and map their regions.
fn preinit_func(dev: &PcieDevice, id: &FuncNum, func: &GeneralFuncConf) -> Result<(), SysError> {
    let mut command_val = func.command();
    command_val.remove(PciCommands::MEM_SPACE | PciCommands::IO_SPACE);
    func.write_command(command_val);

    let mut bars = list_bars(dev, func)?;
    for (bar_idx, bar) in bars.iter_mut() {
        let mut filled = bar.clone();
        filled.set_base_addr(u64::MAX);
        func.write_bar(*bar_idx, filled).map_err(|e| {
            kwarningln!(
                "PCIe device preinit failed: error writing all-1s to BAR #{} for device {}: {:?}",
                bar_idx,
                dev.name(),
                e
            );
            e
        })?;
        *bar = func.read_bar(*bar_idx).map_err(|e| {
            kwarningln!(
                "PCIe device preinit failed: error reading back BAR #{} for device {}: {:?}",
                bar_idx,
                dev.name(),
                e
            );
            e
        })?;
    }
    /*kalertln!(
        "INTR PIN of device {} function {:?} is {}",
        dev.name(),
        id,
        func.intr_pin()
    );
    kalertln!(
        "INTR LINE of device {} function {:?} is {}",
        dev.name(),
        id,
        func.intr_line()
    );*/
    let domain = dev.domain();
    let mut mem_areas: Vec<(
        usize,
        BAR,
        &super::AvailPciMemArea,
        super::PciMemArea,
        IoRemap,
    )> = vec![];
    for (bar_idx, bar) in bars {
        let size = match bar {
            BAR::Memory { base_addr: 0, .. } | BAR::IO { base_addr: 0, .. } => {
                continue;
            },
            BAR::Memory { base_addr, .. } | BAR::IO { base_addr } => {
                ((!(base_addr as u32)) + 1) as u64
            },
        };
        match alloc_mem_for_bar(domain, bar, size) {
            Err(e) => {
                kerrln!(
                    "PCIe device preinit failed: failed to allocate memory for device {} BAR #{}({:?} bytes requested): {:?}",
                    dev.name(),
                    bar_idx,
                    size,
                    e
                );
                for (_, _, areas, area, remap) in mem_areas.into_iter() {
                    unsafe {
                        areas.free(area);
                    }
                    drop(remap);
                }
                return Err(e);
            },
            Ok((areas, area, remap)) => {
                /*kinfoln!(
                    "preinit: allocated memory for device {} BAR #{}: {:?} bytes at PCI address {:?} (mapped to {:?})",
                    dev.name(),
                    bar_idx,
                    size,
                    area.pci_addr(),
                    area.phys_addr()
                );*/
                mem_areas.push((bar_idx, bar, areas, area, remap));
            },
        }
    }
    for (bar_idx, mut bar, areas, area, remap) in mem_areas.into_iter() {
        bar.set_base_addr(area.pci_addr().address());
        func.write_bar(bar_idx, bar);
        add_remap_region(remap);
    }

    Ok(())
}

/// Pre-initialize PCIe device by iterating its functions and calling
/// `preinit_func`.
pub fn preinit_pcie_dev(dev: &PcieDevice) -> Result<(), SysError> {
    let mut bar_idx = 0;
    let Some(dev_conf) = dev.dev_conf() else {
        return Ok(());
    };
    dev_conf.functions::<_, SysError>(|id, func| {
        preinit_func(dev, &id, &func).map_err(|e| {
            kerrln!(
                "preinit failed for PCIe device {}, function {:?}: {:?}",
                dev.name(),
                id,
                e
            );
            e
        })?;
        Ok(())
    });
    Ok(())
}

impl BusType for PcieBusType {
    fn base(&self) -> &BusTypeBase {
        &self.busty_base
    }

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

    fn register_device(&self, device: Arc<dyn Device>) {
        for driver in BusType::base(self).drivers.read_irqsave().iter() {
            if self.matches(device.as_ref(), driver.as_ref()) {
                // TODO: probe defer
                kinfoln!(
                    "initializing pcie bus device {} with driver {}",
                    device.name(),
                    driver.name()
                );
                if let Err(e) = preinit_pcie_dev(
                    device
                        .as_pcie_device()
                        .expect("pcie driver should only be probed with pcie device"),
                ) {
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

                kinfoln!(
                    "probing pcie bus device {} with driver {}",
                    device.name(),
                    driver.name()
                );
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
        BusType::base(self)
            .devices
            .write_irqsave()
            .add_kobject(device);
    }
}
