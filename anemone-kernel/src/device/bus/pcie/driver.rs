use core::mem;

use alloc::sync::Arc;

use crate::{
    device::{
        Device,
        bus::pcie::{
            DeviceBarInfo, PciFunctionIdentifier, PcieDevice,
            domain::{AvailableApertures, PcieAperture, PcieDomain, PcieMemArea},
            ecam::{FuncConf, PciBar, PciClassCode, PciMemBarType},
            remap::add_remap_region,
        },
        kobject::KObject,
    },
    mm::remap::{IoRemap, ioremap},
    prelude::*,
};

/// PCIe driver interface for device matching and initialization.
pub trait PcieDriver: Driver {
    /// Supported class codes. Vendor-device matches take priority when both
    /// tables are non-empty.
    fn class_code_table(&self) -> &[PciClassCode];

    /// Supported `(vendor_id, device_id)` pairs. Vendor-device matches take
    /// priority when both tables are non-empty.
    fn vendor_device_table(&self) -> &[(u16, u16)];

    /// Pre-initialization hook. For bus devices this typically enumerates and
    /// registers child devices.
    fn preinit(&self, _device: Arc<dyn Device>) -> Result<(), SysError> {
        Ok(())
    }

    /// Post-initialization hook.
    fn postinit(&self, _device: Arc<dyn Device>) -> Result<(), SysError> {
        Ok(())
    }

    /// Allocate BARs and other resources for a device.
    fn alloc_resources(
        &self,
        device: Arc<dyn Device>,
        resources: &AvailableApertures,
    ) -> Result<(), SysError> {
        let pdev = device
            .as_pcie_device()
            .expect("pcie driver should only be initialized with pcie device");
        let res = alloc_bars_for_device(pdev, resources)?;
        for bar in &res {
            add_remap_region(&bar.remap);
        }
        pdev.set_bar_info(res);
        Ok(())
    }

    /// Clean up after a failed probe.
    fn fail(&self, device: &dyn Device) {
        // todo: dispose allocated resources
        device.set_driver(None);
    }
}

/// Enumerate BARs for a function, returning `(index, bar_descriptor)` pairs.
pub fn list_bars(
    func_id: PciFunctionIdentifier,
    func: &FuncConf,
) -> Result<Vec<(usize, PciBar)>, SysError> {
    let bar_count = func.bar_count().map_err(|e| {
        kwarningln!(
            "PCIe: error reading BAR count for device {:?}: {:?}",
            func_id,
            e
        );
        e
    })?;
    let mut bar_idx = 0;
    let mut res = vec![];
    while bar_idx < bar_count {
        let bar = func.read_bar(bar_idx).map_err(|e| {
            kwarningln!(
                "PCIe: error reading BAR #{} for device {:?}: {:?}",
                bar_idx,
                func_id,
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

/// Allocate and ioremap a memory region for `bar` of `size` bytes.
fn alloc_mem_for_bar<'a>(
    domain: &PcieDomain,
    bar: PciBar,
    size: u64,
    resources: &'a AvailableApertures,
) -> Result<(&'a PcieAperture, PcieMemArea, IoRemap), SysError> {
    let (areas, area) = resources
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

/// Probe BAR sizes by writing all-1s, allocate aperture space, and write back
/// the assigned addresses. Returns a list of [`DeviceBarInfo`] ready for MMIO
/// remapping.
///
/// The sizing protocol: write `base=~0` to each BAR, read back to determine the
/// number of address bits actually implemented, then compute `size = ~mask +
/// 1`.
pub fn alloc_bars_for_device(
    dev: &PcieDevice,
    resources: &AvailableApertures,
) -> Result<Vec<DeviceBarInfo>, SysError> {
    pub struct ManagedBARArea<'a> {
        aperture: &'a PcieAperture,
        func_conf: &'a FuncConf,
        bar_idx: usize,
        bar: PciBar,
        pcie_area: PcieMemArea,
        remap: Option<IoRemap>,
    }

    impl<'a> ManagedBARArea<'a> {
        pub fn new(
            aperture: &'a PcieAperture,
            pcie_area: PcieMemArea,
            func_conf: &'a FuncConf,
            bar_idx: usize,
            bar: PciBar,
            remap: IoRemap,
        ) -> Result<Self, SysError> {
            Ok(Self {
                aperture,
                pcie_area: pcie_area,
                func_conf,
                bar_idx,
                bar,
                remap: Some(remap),
            })
        }

        pub fn into_leaked(mut self) -> DeviceBarInfo {
            let mut remap = None;
            core::mem::swap(&mut remap, &mut self.remap);
            let res = DeviceBarInfo {
                bar_idx: self.bar_idx,
                bar: self.bar,
                mem_area: self.pcie_area.clone(),
                remap: Arc::new(remap.expect("a managed bar area should have a remap")),
            };
            mem::forget(self);
            res
        }
    }

    impl Drop for ManagedBARArea<'_> {
        fn drop(&mut self) {
            unsafe {
                self.aperture.free(self.pcie_area.clone());
            }
        }
    }
    let func_conf = dev
        .func_conf()
        .expect("PCIe endpoint device must have a configuration space");
    let func_id = dev
        .identifier()
        .expect("PCIe endpoint device must have an identifier");
    let mut bars = list_bars(func_id, func_conf)?;
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
    let mut mem_areas: Vec<ManagedBARArea> = vec![];
    for (bar_idx, bar) in bars {
        let size = match bar {
            PciBar::Memory { base_addr: 0, .. } | PciBar::IO { base_addr: 0, .. } => {
                continue;
            },
            PciBar::Memory { base_addr, .. } | PciBar::IO { base_addr } => {
                // BAR size = ~(lower 32 bits of probed addr) + 1
                ((!(base_addr as u32)) + 1) as u64
            },
        };
        match alloc_mem_for_bar(domain, bar, size, resources) {
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
                mem_areas.push(ManagedBARArea::new(
                    areas, area, func_conf, bar_idx, bar, remap,
                )?);
            },
        }
    }
    for area in mem_areas.iter_mut() {
        area.bar.set_base_addr(area.pcie_area.pci_addr().address());
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
    Ok(mem_areas.into_iter().map(|x| x.into_leaked()).collect())
}
