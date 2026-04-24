use core::{
    mem::{self},
    u64,
};

use kernel_macros::KObject;

use crate::{
    device::{
        Device,
        bus::{
            BusType, BusTypeBase,
            pcie::{
                AvailPciMemArea, PciMemArea, PcieDevice, PcieDomain,
                ecam::{FuncConf, PciBar, PciCommands, PciMemBarType},
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
fn list_bars(dev: &PcieDevice, func: &FuncConf) -> Result<Vec<(usize, PciBar)>, SysError> {
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
            PciBar::Memory {
                mtype: PciMemBarType::W64,
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
    bar: PciBar,
    size: u64,
) -> Result<(&AvailPciMemArea, PciMemArea, IoRemap), SysError> {
    let (areas, area) = domain
        .resources()
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

/// Pre-initialize a device on the PCIe bus by enumerating and claiming its
/// BARs.
pub fn preinit_device(dev: &PcieDevice) -> Result<(), SysError> {
    let Some(func_conf) = dev.func_conf() else {
        // host bridge
        return Ok(());
    };

    // clear commands
    func_conf.write_command(PciCommands::empty());

    pub struct ManagedArea<'a> {
        avail_areas: &'a AvailPciMemArea,
        func_conf: &'a FuncConf,
        bar_idx: usize,
        bar: PciBar,
        pci_area: PciMemArea,
        remap: Option<IoRemap>,
    }

    impl<'a> ManagedArea<'a> {
        pub fn new(
            avail_areas: &'a AvailPciMemArea,
            pci_area: PciMemArea,
            func_conf: &'a FuncConf,
            bar_idx: usize,
            bar: PciBar,
            remap: IoRemap,
        ) -> Result<Self, SysError> {
            Ok(Self {
                avail_areas,
                pci_area,
                func_conf,
                bar_idx,
                bar,
                remap: Some(remap),
            })
        }

        pub fn add_remap_and_forget(mut self) {
            let mut remap = None;
            mem::swap(&mut self.remap, &mut remap);
            add_remap_region(remap.unwrap());
            mem::forget(self);
        }
    }

    impl Drop for ManagedArea<'_> {
        fn drop(&mut self) {
            unsafe {
                self.avail_areas.free(self.pci_area.clone());
            }
        }
    }

    let mut bars = list_bars(dev, func_conf)?;
    for (bar_idx, bar_value) in bars.iter_mut() {
        let mut filled = bar_value.clone();
        filled.set_base_addr(u64::MAX);
        func_conf.write_bar(*bar_idx, filled).map_err(|e| {
            kwarningln!(
                "PCIe device preinit failed: error writing all-1s to BAR #{} for device {}: {:?}",
                bar_idx,
                dev.name(),
                e
            );
            e
        })?;
        *bar_value = func_conf.read_bar(*bar_idx).map_err(|e| {
            kwarningln!(
                "PCIe device preinit failed: error reading back BAR #{} for device {}: {:?}",
                bar_idx,
                dev.name(),
                e
            );
            e
        })?;
    }
    let domain = dev.domain();
    let mut mem_areas: Vec<ManagedArea> = vec![];
    for (bar_idx, bar) in bars {
        let size = match bar {
            PciBar::Memory { base_addr: 0, .. } | PciBar::IO { base_addr: 0, .. } => {
                continue;
            },
            PciBar::Memory { base_addr, .. } | PciBar::IO { base_addr } => {
                // upper bits are ignored
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
                mem_areas.push(ManagedArea::new(
                    areas, area, func_conf, bar_idx, bar, remap,
                )?);
            },
        }
    }
    for area in mem_areas.iter_mut() {
        area.bar.set_base_addr(area.pci_area.pci_addr().address());
        func_conf.write_bar(area.bar_idx, area.bar).map_err(|e| {
            kerrln!(
                "PCIe device preinit failed: error writing BAR #{} for device {}: {:?}",
                area.bar_idx,
                dev.name(),
                e
            );
            e
        })?;
    }
    for area in mem_areas.into_iter() {
        area.add_remap_and_forget();
    }
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
                if let Err(e) = preinit_device(
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
