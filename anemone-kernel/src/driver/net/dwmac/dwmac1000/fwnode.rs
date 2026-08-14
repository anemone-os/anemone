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
    pub(super) dma: LegacyDmaConfig,
    pub(super) axi: Option<LegacyAxiConfig>,
    pub(super) ps_speed: Option<u32>,
    pub(super) max_speed: Option<u32>,
    pub(super) max_mtu: Option<u32>,
    pub(super) mdio_clock_range: Option<MdioClockRange>,
    pub(super) clocks_present: bool,
    pub(super) resets_present: bool,
    pub(super) pinctrl_present: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LegacyDmaConfig {
    pub(super) tx_pbl: LegacyBurstLength,
    pub(super) rx_pbl: LegacyBurstLength,
    pub(super) pbl_x8: bool,
    pub(super) fixed_burst: bool,
    pub(super) mixed_burst: bool,
    pub(super) address_aligned_beats: bool,
    pub(super) operation_mode: DmaOperationMode,
    pub(super) rx_fifo_bytes: Option<u32>,
    pub(super) tx_fifo_bytes: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LegacyAxiConfig {
    pub(super) lpi_enable: bool,
    pub(super) exit_frame: bool,
    /// Parsed binding inputs that Linux 6.6's legacy DWMAC1000 AXI callback
    /// does not consume. They are diagnostic crop facts, never register policy.
    pub(super) unused_binding_flags: u8,
    pub(super) write_outstanding_limit: u8,
    pub(super) read_outstanding_limit: u8,
    pub(super) burst_mask: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DmaOperationMode {
    ForceThreshold,
    ForceStoreForward,
    HardwareDefault,
}

impl DmaOperationMode {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::ForceThreshold => "force-threshold",
            Self::ForceStoreForward => "force-store-forward",
            Self::HardwareDefault => "hardware-capability-default",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LegacyBurstLength(u8);

impl LegacyBurstLength {
    const fn new(value: u32) -> Option<Self> {
        if matches!(value, 1 | 2 | 4 | 8 | 16 | 32) {
            Some(Self(value as u8))
        } else {
            None
        }
    }

    pub(super) const fn encoded(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct MdioClockRange(u8);

impl MdioClockRange {
    pub(super) const fn new(value: u32) -> Option<Self> {
        if value <= 5 || (value >= 8 && value <= 15) {
            Some(Self(value as u8))
        } else {
            None
        }
    }

    pub(super) const fn encoded(self) -> u8 {
        self.0
    }
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
        // `dma-mask` is diagnostic firmware input only. Legacy descriptor/base
        // width is proven from every actual allocation range below 4 GiB.
        let dma_mask = fwnode.prop_read_u64("dma-mask").unwrap_or(u64::MAX);
        let pbl = LegacyBurstLength::new(fwnode.prop_read_u32("snps,pbl").unwrap_or(8))
            .ok_or(SysError::DriverIncompatible)?;
        let force_threshold = fwnode.prop_read_present("snps,force_thresh_dma_mode");
        let force_store_forward = fwnode.prop_read_present("snps,force_sf_dma_mode");
        // Linux gives threshold mode precedence when both DT flags are
        // present. Preserve that ABI choice here instead of admitting an
        // otherwise contradictory policy into the register owner.
        let operation_mode = if force_threshold {
            DmaOperationMode::ForceThreshold
        } else if force_store_forward {
            DmaOperationMode::ForceStoreForward
        } else {
            DmaOperationMode::HardwareDefault
        };
        let dma = LegacyDmaConfig {
            tx_pbl: LegacyBurstLength::new(
                fwnode
                    .prop_read_u32("snps,txpbl")
                    .unwrap_or(pbl.encoded() as u32),
            )
            .ok_or(SysError::DriverIncompatible)?,
            rx_pbl: LegacyBurstLength::new(
                fwnode
                    .prop_read_u32("snps,rxpbl")
                    .unwrap_or(pbl.encoded() as u32),
            )
            .ok_or(SysError::DriverIncompatible)?,
            pbl_x8: !fwnode.prop_read_present("snps,no-pbl-x8"),
            fixed_burst: fwnode.prop_read_present("snps,fixed-burst"),
            mixed_burst: fwnode.prop_read_present("snps,mixed-burst"),
            address_aligned_beats: fwnode.prop_read_present("snps,aal"),
            operation_mode,
            rx_fifo_bytes: nonzero_property(fwnode.prop_read_u32("rx-fifo-depth")),
            tx_fifo_bytes: nonzero_property(fwnode.prop_read_u32("tx-fifo-depth")),
        };
        if fwnode.prop_read_present("snps,mtl-rx-config")
            || fwnode.prop_read_present("snps,mtl-tx-config")
        {
            // Gate 2 deliberately supports the Linux legacy one-queue path.
            // A queue configuration phandle is a different admitted target,
            // not permission to ignore firmware topology.
            return Err(SysError::DriverIncompatible);
        }
        let axi = parse_axi_config(node.node())?;
        let ps_speed = fwnode
            .prop_read_u32("snps,ps-speed")
            .map(|speed| {
                if matches!(speed, 10 | 100 | 1000) {
                    Ok(speed)
                } else {
                    Err(SysError::DriverIncompatible)
                }
            })
            .transpose()?;
        let max_speed = fwnode
            .prop_read_u32("max-speed")
            .map(|speed| {
                if matches!(speed, 10 | 100 | 1000) {
                    Ok(speed)
                } else {
                    Err(SysError::DriverIncompatible)
                }
            })
            .transpose()?;
        let max_mtu = fwnode.prop_read_u32("max-frame-size");
        if max_mtu.is_some_and(|mtu| mtu < 1500) {
            return Err(SysError::DriverIncompatible);
        }
        let firmware_mdio_clock = fwnode
            .prop_read_u32("snps,clk-csr")
            .or_else(|| fwnode.prop_read_u32("clk_csr"));
        let mdio_clock_range = firmware_mdio_clock
            .map(|value| MdioClockRange::new(value).ok_or(SysError::DriverIncompatible))
            .transpose()?;
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
            dma,
            axi,
            ps_speed,
            max_speed,
            max_mtu,
            mdio_clock_range,
            clocks_present: fwnode.prop_read_present("clocks"),
            resets_present: fwnode.prop_read_present("resets"),
            pinctrl_present: fwnode.prop_read_present("pinctrl-0"),
        })
    }
}

fn parse_axi_config(node: &device_tree::DeviceNode) -> Result<Option<LegacyAxiConfig>, SysError> {
    let Some(phandle) = node
        .property("snps,axi-config")
        .and_then(|property| property.value_as_phandle())
    else {
        return Ok(None);
    };
    of_with_node_by_phandle(phandle, |axi| {
        let read_limit = axi
            .property("snps,rd_osr_lmt")
            .and_then(|property| property.value_as_u32())
            .unwrap_or(1);
        let write_limit = axi
            .property("snps,wr_osr_lmt")
            .and_then(|property| property.value_as_u32())
            .unwrap_or(1);
        if read_limit > 0xf || write_limit > 0xf {
            return Err(SysError::DriverIncompatible);
        }
        let burst_mask = axi
            .property("snps,blen")
            .map(|property| {
                let values = property
                    .value_as_u32_array()
                    .ok_or(SysError::DriverIncompatible)?;
                let mut count = 0usize;
                let mut mask = 0u8;
                for value in values.iter() {
                    count += 1;
                    mask |= match value {
                        0 => 0,
                        4 => 1 << 1,
                        8 => 1 << 2,
                        16 => 1 << 3,
                        32 => 1 << 4,
                        64 => 1 << 5,
                        128 => 1 << 6,
                        256 => 1 << 7,
                        _ => return Err(SysError::DriverIncompatible),
                    };
                }
                if count != 7 {
                    return Err(SysError::DriverIncompatible);
                }
                Ok(mask)
            })
            .transpose()?
            .unwrap_or(0);
        let present = |name: &str| axi.properties().any(|property| property.name() == name);
        Ok(LegacyAxiConfig {
            lpi_enable: present("snps,lpi_en"),
            exit_frame: present("snps,xit_frm"),
            unused_binding_flags: (present("snps,kbbe") as u8)
                | ((present("snps,fb") as u8) << 1)
                | ((present("snps,mb") as u8) << 2)
                | ((present("snps,rb") as u8) << 3),
            write_outstanding_limit: write_limit as u8,
            read_outstanding_limit: read_limit as u8,
            burst_mask,
        })
    })
    .map_err(|_| SysError::MissingFwNode)?
    .map(Some)
}

const fn nonzero_property(value: Option<u32>) -> Option<u32> {
    match value {
        Some(value) if value != 0 => Some(value),
        _ => None,
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

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn legacy_burst_length_accepts_only_linux_dt_encodings() {
        for value in [1, 2, 4, 8, 16, 32] {
            assert_eq!(
                LegacyBurstLength::new(value).unwrap().encoded(),
                value as u8
            );
        }
        for value in [0, 3, 5, 31, 33] {
            assert!(LegacyBurstLength::new(value).is_none());
        }
    }

    #[kunit]
    fn legacy_mdio_clock_range_rejects_reserved_encodings() {
        for value in [0, 1, 2, 3, 4, 5, 8, 9, 10, 11, 12, 13, 14, 15] {
            assert_eq!(MdioClockRange::new(value).unwrap().encoded(), value as u8);
        }
        assert!(MdioClockRange::new(6).is_none());
        assert!(MdioClockRange::new(7).is_none());
        assert!(MdioClockRange::new(16).is_none());
    }

    #[kunit]
    fn zero_fifo_depth_has_linux_missing_property_semantics() {
        assert_eq!(nonzero_property(None), None);
        assert_eq!(nonzero_property(Some(0)), None);
        assert_eq!(nonzero_property(Some(4096)), Some(4096));
    }

    #[kunit]
    fn dma_operation_mode_names_are_stable_evidence_values() {
        assert_eq!(DmaOperationMode::ForceThreshold.name(), "force-threshold");
        assert_eq!(
            DmaOperationMode::ForceStoreForward.name(),
            "force-store-forward"
        );
        assert_eq!(
            DmaOperationMode::HardwareDefault.name(),
            "hardware-capability-default"
        );
    }
}
