use kernel_macros::{Driver, KObject};

use crate::{
    device::{
        bus::{
            pcie::{
                self, AvailPciMemArea, OfPciAddr, PciAddrFlags, PcieDevice, PcieDomain,
                ecam::{BusNum, EcamConf},
            },
            platform::{self, PlatformDriver},
        },
        discovery::fwnode::FwNode,
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
        resource::Resource,
    },
    driver::{DriverBase, DriverOps},
    mm::remap::ioremap,
    prelude::*,
};

/// [DOMAINS] is a global allocator for unique PCIe domain identifiers.
static DOMAINS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, KObject, Driver)]
struct PcieEcamDriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for PcieEcamDriver {}

impl DriverOps for PcieEcamDriver {
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
        let (root_bus_num_u8, max_bus_num_u8) = of_node
            .prop_read_raw("bus-range")
            .and_then(|arr| {
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

        let domain_id = DOMAINS.fetch_add(1, Ordering::SeqCst);
        let root_bus_num = BusNum::try_from(root_bus_num_u8).unwrap();
        let max_bus_num = BusNum::try_from(max_bus_num_u8).unwrap();
        let ident =
            KObjIdent::try_from_fmt(format_args!("pci{:04x}:{:02x}", domain_id, root_bus_num_u8))
                .unwrap();

        let remap = unsafe { ioremap(base, len) }?;
        let regs = unsafe { EcamConf::new(&remap, root_bus_num, max_bus_num)? };
        let mut domain = PcieDomain::new(domain_id, regs);

        // ranges

        let cells = of_node.node().cells();
        if cells.addr_cells != 3 {
            kerrln!(
                "error probing PCIe ECAM device {}: expected #address-cells=3, got {}",
                pdev.name(),
                cells.addr_cells
            );
            return Err(SysError::InvalidArgument);
        }
        let size_cells = cells.size_cells;
        let addr_cells_parent = of_node.node().cells_self().addr_cells;
        let range_item_width = size_cells + addr_cells_parent + 3;
        let ranges = of_node.node().ranges().ok_or_else(|| {
            kerrln!(
                "error probing PCIe ECAM device {}: missing 'ranges' property",
                pdev.name()
            );
            SysError::InvalidArgument
        })?;
        let ranges_raw = ranges.raw();
        if ranges_raw.len() % (range_item_width as usize * 4) != 0 {
            kerrln!(
                "error probing PCIe ECAM device {}: 'ranges' property has invalid length {}",
                pdev.name(),
                ranges_raw.len()
            );
            return Err(SysError::InvalidArgument);
        }

        let count = ranges_raw.len() / (range_item_width as usize * 4);
        for i in 0..count {
            let pcie_addr = &ranges_raw[(i * range_item_width as usize * 4)..][0..12];
            let mem_addr = &ranges_raw[(i * range_item_width as usize * 4)..]
                [12..12 + addr_cells_parent as usize * 4];
            let size = &ranges_raw[(i * range_item_width as usize * 4)..]
                [12 + addr_cells_parent as usize * 4..][..size_cells as usize * 4];
            let mem_addr = (0..addr_cells_parent)
                .map(|j| u32::from_be_bytes(mem_addr[(j as usize * 4)..][..4].try_into().unwrap()))
                .fold(0u64, |acc, x| (acc << 32) | x as u64);
            let size = (0..size_cells)
                .map(|j| u32::from_be_bytes(size[(j as usize * 4)..][..4].try_into().unwrap()))
                .fold(0u64, |acc, x| (acc << 32) | x as u64);
            let pcie_addr = OfPciAddr::from_be_bytes(pcie_addr.try_into().unwrap());
            if !pcie_addr.flags().contains(PciAddrFlags::NotRelocatable) {
                /*knoticeln!(
                    "Available PCIe ECAM resource window: pcie_addr={:?}, mem_addr={:#x}, size={:#x}",
                    pcie_addr,
                    mem_addr,
                    size
                );*/
                domain.add_mem_range(AvailPciMemArea::new(
                    pcie_addr,
                    PhysAddr::new(mem_addr),
                    size,
                )).map_err(|e|{
                    kerrln!(
                        "error probing PCIe ECAM device {}: invalid memory range in 'ranges' property: {:?}",
                        pdev.name(),
                        e
                    );
                    e
                })?;
            }
        }

        let domain = Arc::new(domain);
        let device = PcieDevice::new_host_bridge(ident, domain, root_bus_num);
        let device = Arc::new(device);
        pcie::register_device(device.clone());
        device.set_parent(Some(ROOT.clone()));
        ROOT.add_child(device);
        Ok(())
    }

    fn as_platform_driver(&self) -> Option<&dyn PlatformDriver> {
        Some(self)
    }

    fn shutdown(&self, device: &dyn Device) {
        // todo
    }
}

impl PlatformDriver for PcieEcamDriver {
    /// [match_table] declares Open Firmware compatible strings handled by this
    /// driver.
    fn match_table(&self) -> &[&str] {
        &["pci-host-ecam-generic"]
    }
}

#[initcall(driver)]
fn init() {
    let kobj_base = KObjectBase::new(KObjIdent::try_from("pci-host-ecam-generic").unwrap());
    let drv_base = DriverBase::new();
    let driver = Arc::new(PcieEcamDriver {
        kobj_base,
        drv_base,
    });

    platform::register_driver(driver);
}
