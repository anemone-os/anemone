use crate::{
    device::{
        bus::platform::PlatformDevice,
        discovery::{
            fwnode::{InterruptSelector, select_interrupt_resource},
            open_firmware::of_with_node_by_phandle,
        },
        resource::Resource,
    },
    prelude::*,
};

#[derive(Debug)]
pub(super) struct Dwmac1000Config {
    pub(super) node_path: String,
    pub(super) compatible0: String,
    pub(super) compatible1: String,
    pub(super) mmio: (PhysAddr, usize),
    pub(super) interrupt_index: usize,
    pub(super) interrupt_specifier_bytes: usize,
    pub(super) interrupt_specifier: u32,
    pub(super) phy_mode: String,
    pub(super) phy_address: u8,
    pub(super) mac: [u8; 6],
    pub(super) dma_mask: u64,
    pub(super) clocks_present: bool,
    pub(super) resets_present: bool,
    pub(super) pinctrl_present: bool,
}

impl Dwmac1000Config {
    pub(super) fn parse(device: &PlatformDevice) -> Result<Self, SysError> {
        let fwnode = device.fwnode().ok_or(SysError::MissingFwNode)?;
        let node = fwnode.as_of_node().ok_or(SysError::FwNodeLookupFailed)?;
        let node_path = node.node().path();
        let mut compatibles = device.compatibles();
        let compatible0 = compatibles.next().unwrap_or("<missing>").to_string();
        let compatible1 = compatibles.next().unwrap_or("<missing>").to_string();
        let mmio = single_mmio(device.resources()).ok_or(SysError::MissingResource)?;
        let interrupt =
            select_interrupt_resource(fwnode.as_ref(), InterruptSelector::Name("macirq"))
                .map_err(|_| SysError::InvalidInterruptInfo)?;
        let interrupt_specifier = u32::from_be_bytes(
            interrupt
                .specifier()
                .try_into()
                .map_err(|_| SysError::InvalidInterruptInfo)?,
        );
        let phy_mode = fwnode
            .prop_read_str("phy-mode")
            .ok_or(SysError::DriverIncompatible)?;
        if !matches!(phy_mode.as_str(), "rgmii" | "rgmii-id") {
            return Err(SysError::DriverIncompatible);
        }
        let mac = fwnode
            .prop_read_raw("local-mac-address")
            .and_then(|raw| raw.try_into().ok())
            .filter(|mac: &[u8; 6]| mac.iter().any(|byte| *byte != 0) && mac[0] & 1 == 0)
            .ok_or(SysError::DriverIncompatible)?;
        let dma_mask = fwnode
            .prop_read_u64("dma-mask")
            .ok_or(SysError::DriverIncompatible)?;
        let phandle = node
            .node()
            .property("phy-handle")
            .and_then(|property| property.value_as_phandle())
            .ok_or(SysError::MissingFwNode)?;
        let phy_address = of_with_node_by_phandle(phandle, |phy| {
            phy.property("reg")
                .and_then(|property| property.value_as_u32())
                .or_else(|| {
                    phy.unit_addr()
                        .and_then(|value| u32::from_str_radix(value, 16).ok())
                })
        })
        .ok()
        .flatten()
        .and_then(|address| u8::try_from(address).ok())
        .filter(|address| *address <= 0x1f)
        .ok_or(SysError::DriverIncompatible)?;
        Ok(Self {
            node_path,
            compatible0,
            compatible1,
            mmio,
            interrupt_index: interrupt.index(),
            interrupt_specifier_bytes: interrupt.specifier().len(),
            interrupt_specifier,
            phy_mode,
            phy_address,
            mac,
            dma_mask,
            clocks_present: fwnode.prop_read_present("clocks"),
            resets_present: fwnode.prop_read_present("resets"),
            pinctrl_present: fwnode.prop_read_present("pinctrl-0"),
        })
    }
}

fn single_mmio(resources: &[Resource]) -> Option<(PhysAddr, usize)> {
    let mut selected = None;
    for resource in resources {
        let Resource::Mmio { base, len } = resource;
        if *len == 0 || selected.is_some() {
            return None;
        }
        selected = Some((*base, *len));
    }
    selected
}
