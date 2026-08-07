//! Generic PCIe host bridge driver for ECAM-compatible controllers.
//!
//! Probes platform devices matching `"pci-host-ecam-generic"`. Reads the
//! `bus-range`, `ranges`, and `interrupt-map` properties from the Open Firmware
//! node to configure a [`PcieDomain`], then registers the root bus device.

use core::mem::size_of;

use kernel_macros::{Driver, KObject};

use crate::{
    device::{
        bus::{
            pcie::{
                self, OfPciAddr, OfPciAddrFlags, PcieDevice,
                domain::{PcieDomain, PcieIntrInfo, PcieIntrKey},
                ecam::{BusNum, EcamConf},
            },
            platform::{self, PlatformDriver},
        },
        discovery::{
            fwnode::FwNode,
            open_firmware::{get_of_node, of_with_node_by_phandle},
        },
        kobject::{KObjIdent, KObject, KObjectBase, KObjectOps},
        resource::Resource,
    },
    driver::{DriverBase, DriverOps},
    mm::remap::ioremap,
    prelude::*,
};

/// Global allocator for unique PCIe domain identifiers.
static DOMAINS: AtomicUsize = AtomicUsize::new(0);

// QEMU 10.0.2's LoongArch `virt` DT omits the PCI child
// `#interrupt-cells` property and the level-high cell from every PCH-PIC
// parent specifier. Keep this recognition exact so malformed generic PCI DTs
// remain rejected. The compatibility path can be removed once every supported
// QEMU build provider emits the corrected seven-cell entries.
const QEMU_LOONGARCH_LEGACY_MAP_ENTRY_COUNT: usize = 16;
const QEMU_LOONGARCH_LEGACY_MAP_ENTRY_BYTES: usize = 6 * size_of::<u32>();
const QEMU_LOONGARCH_LEGACY_MAP_MASK: [u32; 4] = [0x1800, 0, 0, 7];
const QEMU_LOONGARCH_PCH_LEVEL_HIGH: u32 = 4;

struct QemuLoongArchLegacyInterruptMap {
    normalized: Vec<u8>,
    parent_handle: u32,
}

fn normalize_qemu_loongarch_legacy_interrupt_map(
    intr_map: &[u8],
    intr_mask: Option<&[u8]>,
) -> Option<QemuLoongArchLegacyInterruptMap> {
    let Some(intr_mask) = intr_mask else {
        return None;
    };
    if intr_map.len()
        != QEMU_LOONGARCH_LEGACY_MAP_ENTRY_COUNT * QEMU_LOONGARCH_LEGACY_MAP_ENTRY_BYTES
        || intr_mask.len() != QEMU_LOONGARCH_LEGACY_MAP_MASK.len() * size_of::<u32>()
    {
        return None;
    }
    if !intr_mask
        .chunks_exact(size_of::<u32>())
        .map(|cell| u32::from_be_bytes(cell.try_into().unwrap()))
        .eq(QEMU_LOONGARCH_LEGACY_MAP_MASK)
    {
        return None;
    }

    let mut normalized =
        Vec::with_capacity(QEMU_LOONGARCH_LEGACY_MAP_ENTRY_COUNT * 7 * size_of::<u32>());
    let mut parent_handle = None;
    for (index, entry) in intr_map
        .chunks_exact(QEMU_LOONGARCH_LEGACY_MAP_ENTRY_BYTES)
        .enumerate()
    {
        let slot = index / 4;
        let pin_index = index % 4;
        let child_addr_hi = u32::from_be_bytes(entry[0..4].try_into().unwrap());
        let child_addr_mid = u32::from_be_bytes(entry[4..8].try_into().unwrap());
        let child_addr_lo = u32::from_be_bytes(entry[8..12].try_into().unwrap());
        let child_intr_spec = u32::from_be_bytes(entry[12..16].try_into().unwrap());
        let entry_parent_handle = u32::from_be_bytes(entry[16..20].try_into().unwrap());
        let parent_hwirq = u32::from_be_bytes(entry[20..24].try_into().unwrap());
        if child_addr_hi != (slot as u32) << 11
            || child_addr_mid != 0
            || child_addr_lo != 0
            || child_intr_spec != (pin_index + 1) as u32
            || parent_hwirq != 16 + ((slot + pin_index) % 4) as u32
            || parent_handle.is_some_and(|expected| expected != entry_parent_handle)
        {
            return None;
        }
        parent_handle = Some(entry_parent_handle);
        normalized.extend_from_slice(entry);
        normalized.extend_from_slice(&QEMU_LOONGARCH_PCH_LEVEL_HIGH.to_be_bytes());
    }

    Some(QemuLoongArchLegacyInterruptMap {
        normalized,
        parent_handle: parent_handle.unwrap(),
    })
}

/// Driver for generic ECAM-compatible PCIe host controllers.
#[derive(Debug, KObject, Driver)]
struct PcieEcamDriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

impl KObjectOps for PcieEcamDriver {}

impl DriverOps for PcieEcamDriver {
    /// Probe a platform device as a PCIe ECAM host bridge.
    ///
    /// Parses the OF node's `bus-range`, `ranges` (address space windows), and
    /// `interrupt-map` (IRQ routing) to build a [`PcieDomain`]. Registers the
    /// root bus device.
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
        let ecam = unsafe { EcamConf::new(&remap, root_bus_num, max_bus_num)? };
        let mut domain = PcieDomain::new(ecam, root_bus_num, max_bus_num);

        // Parse "ranges" property: translate PCIe address space windows to
        // physical memory ranges and register them with the domain.
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
            if !pcie_addr.flags().contains(OfPciAddrFlags::NotRelocatable) {
                knoticeln!(
                    "Available PCIe ECAM resource window: pcie_addr={:?}, mem_addr={:#x}, size={:#x}",
                    pcie_addr,
                    mem_addr,
                    size
                );
                domain.resources_mut().add_mem_range(
                    pcie_addr,
                    PhysAddr::new(mem_addr),
                    size,
                ).map_err(|e|{
                    kerrln!(
                        "error probing PCIe ECAM device {}: invalid memory range in 'ranges' property: {:?}",
                        pdev.name(),
                        e
                    );
                    e
                })?;
            }
        }

        // Parse "interrupt-map" property: translate PCIe INTx pins to platform
        // interrupt specifiers and register them with the domain.
        let intr_map_raw = of_node.prop_read_raw("interrupt-map").ok_or_else(|| {
            kerrln!(
                "error probing PCIe ECAM device {}: 'interrupt-map' not specified.",
                pdev.name()
            );
            SysError::InvalidArgument
        })?;
        let intr_mask = of_node.prop_read_raw("interrupt-map-mask");
        let legacy_qemu_loongarch = of_node
            .node()
            .interrupt_cells()
            .is_none()
            .then(|| normalize_qemu_loongarch_legacy_interrupt_map(intr_map_raw, intr_mask))
            .flatten();
        let intr_cells = match of_node.node().interrupt_cells() {
            Some(intr_cells) => intr_cells as usize,
            None if legacy_qemu_loongarch.is_some() => 1,
            None => {
                kerrln!(
                    "error probing PCIe ECAM device {}: '#interrupt-cells' not specified.",
                    pdev.name()
                );
                return Err(SysError::InvalidArgument);
            },
        };
        let intr_map = legacy_qemu_loongarch
            .as_ref()
            .map_or(intr_map_raw, |legacy| legacy.normalized.as_slice());

        if intr_cells == 0 || intr_cells > 2 {
            kerrln!(
                "error probing PCIe ECAM device {}: invalid 'interrupt-cells' value {}.",
                pdev.name(),
                intr_cells
            );
            return Err(SysError::InvalidArgument);
        }

        if let Some(intr_mask) = intr_mask {
            if intr_mask.len() != (3 + intr_cells) * 4 {
                kerrln!(
                    "error probing PCIe ECAM device {}: invalid 'interrupt-map-mask' length {}.",
                    pdev.name(),
                    intr_mask.len()
                );
                return Err(SysError::InvalidArgument);
            }
            let addr = OfPciAddr::from_be_bytes(intr_mask[0..12].try_into().unwrap());
            let specifier_raw = &intr_mask[12..];
            let mut specifier = 0;
            for i in 0..intr_cells {
                specifier <<= 32;
                specifier |=
                    u32::from_be_bytes(specifier_raw[(i as usize * 4)..][..4].try_into().unwrap())
                        as u64;
            }
            domain.resources_mut().set_intr_key_mask(PcieIntrKey {
                func_addr: addr.func_addr(),
                intr_pin: specifier as u8,
            });
        }

        // Lower half of an interrupt-map entry: 3 cells child address +
        // intr_cells child specifier + 1 cell interrupt parent phandle.
        let intr_map_item_width_half = ((3 + intr_cells + 1) * 4) as usize;
        let mut index = 0;
        while index + intr_map_item_width_half <= intr_map.len() {
            let lower_half = &intr_map[index..][..intr_map_item_width_half];

            let child_addr = &lower_half[0..12];
            let child_addr = OfPciAddr::from_be_bytes(child_addr.try_into().unwrap());

            let child_intr_spec_raw = &lower_half[12..][..intr_cells * 4];

            let mut child_intr_spec = 0;
            for i in 0..intr_cells {
                child_intr_spec <<= 32;
                child_intr_spec |= u32::from_be_bytes(
                    child_intr_spec_raw[(i as usize * 4)..][..4]
                        .try_into()
                        .unwrap(),
                ) as u64;
            }

            let intr_parent_handle =
                u32::from_be_bytes(lower_half[12 + intr_cells * 4..].try_into().unwrap());
            let parent_node = of_with_node_by_phandle(intr_parent_handle, |node| node.handle())
                .ok()
                .map(get_of_node)
                .ok_or_else(||{
                    kerrln!(
                        "error probing PCIe ECAM device {}: failed to lookup interrupt parent node with phandle {:#x}.",
                        pdev.name(),
                        intr_parent_handle
                    );
                    SysError::FwNodeLookupFailed
                }
            )?;

            if legacy_qemu_loongarch.as_ref().is_some_and(|legacy| {
                legacy.parent_handle != intr_parent_handle
                    || !parent_node
                        .node()
                        .compatible()
                        .is_some_and(|mut compatible| {
                            compatible.any(|entry| entry == "loongson,pch-pic-1.0")
                        })
                    || parent_node.node().address_cells_or_none().unwrap_or(0) != 0
                    || parent_node.node().interrupt_cells() != Some(2)
            }) {
                kerrln!(
                    "error probing PCIe ECAM device {}: QEMU LoongArch legacy interrupt parent {:#x} is not its single two-cell PCH-PIC domain.",
                    pdev.name(),
                    intr_parent_handle
                );
                return Err(SysError::InvalidArgument);
            }

            // QEMU's interrupt controller sets #address-cells=0, which
            // deviates from the DTB spec; use 0 as the fallback.
            let (addr_cells_par, intr_cells_par) =
            (
                parent_node.node().address_cells_or_none().unwrap_or(0) as usize,
                parent_node.node().interrupt_cells().ok_or_else(||{
                    kerrln!(
                        "error probing PCIe ECAM device {}: '#interrupt-cells' not specified for interrupt parent {:#x}.",
                        pdev.name(),
                        intr_parent_handle
                    );
                    SysError::InvalidArgument
                })? as usize
            );

            let intr_map_item_width_half_upper = (addr_cells_par + intr_cells_par) * 4;
            let upper_half = &intr_map[index + intr_map_item_width_half..];
            if upper_half.len() < intr_map_item_width_half_upper {
                kerrln!(
                    "error probing PCIe ECAM device {}: incomplete interrupt map entry for child address {:?}.",
                    pdev.name(),
                    child_addr
                );
                return Err(SysError::InvalidArgument);
            }

            let parent_addr = &upper_half[0..addr_cells_par * 4];

            let parent_intr_spec = &upper_half[addr_cells_par * 4..][..intr_cells_par * 4];
            domain.resources_mut().add_intr_map(
                PcieIntrKey {
                    func_addr: child_addr.func_addr(),
                    intr_pin: child_intr_spec as u8,
                },
                PcieIntrInfo {
                    parent: parent_node,
                    parent_intr_spec: Box::from(parent_intr_spec),
                },
            );
            index += intr_map_item_width_half + intr_map_item_width_half_upper;
        }
        if legacy_qemu_loongarch.is_some() {
            knoticeln!(
                "PCIe ECAM device {} accepted the QEMU 10.0.2 LoongArch legacy interrupt-map",
                pdev.name()
            );
        }

        let domain = Arc::new(domain);
        let device = PcieDevice::new_bus(ident, domain, root_bus_num);
        let device = Arc::new(device);
        pcie::register_device(device.clone());
        device.set_parent(Some(ROOT.clone()));
        ROOT.add_child(device);
        Ok(())
    }

    fn as_platform_driver(&self) -> Option<&dyn PlatformDriver> {
        Some(self)
    }

    fn shutdown(&self, _device: &dyn Device) {}
}

impl PlatformDriver for PcieEcamDriver {
    /// OF compatible strings matched by this driver.
    fn match_table(&self) -> &[&str] {
        &["pci-host-ecam-generic"]
    }
}

/// Create and register the [`PcieEcamDriver`] singleton.
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

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn cells(values: &[u32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_be_bytes())
            .collect()
    }

    #[kunit]
    fn qemu_loongarch_legacy_map_requires_the_exact_container_shape() {
        let mut map = Vec::new();
        for index in 0..QEMU_LOONGARCH_LEGACY_MAP_ENTRY_COUNT {
            let slot = index / 4;
            let pin_index = index % 4;
            map.extend_from_slice(&cells(&[
                (slot as u32) << 11,
                0,
                0,
                (pin_index + 1) as u32,
                0x800a,
                16 + ((slot + pin_index) % 4) as u32,
            ]));
        }
        let mask = cells(&QEMU_LOONGARCH_LEGACY_MAP_MASK);
        let normalized = normalize_qemu_loongarch_legacy_interrupt_map(&map, Some(&mask)).unwrap();
        assert_eq!(normalized.parent_handle, 0x800a);
        assert_eq!(normalized.normalized.len(), 16 * 7 * size_of::<u32>());
        for entry in normalized.normalized.chunks_exact(7 * size_of::<u32>()) {
            assert_eq!(&entry[24..28], &QEMU_LOONGARCH_PCH_LEVEL_HIGH.to_be_bytes());
        }

        assert!(
            normalize_qemu_loongarch_legacy_interrupt_map(&map[..map.len() - 1], Some(&mask))
                .is_none()
        );
        let wrong_mask = cells(&[0x1800, 0, 0, 3]);
        assert!(normalize_qemu_loongarch_legacy_interrupt_map(&map, Some(&wrong_mask)).is_none());
        assert!(normalize_qemu_loongarch_legacy_interrupt_map(&map, None).is_none());
    }
}
