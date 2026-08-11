use crate::{
    device::{bus::platform::PlatformDevice, discovery::fwnode::FwNode, resource::Resource},
    prelude::*,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GmacPhyConfig {
    pub(super) address: u8,
    pub(super) rxc_delay_enable: Option<bool>,
    pub(super) rgmii_drive: u8,
    pub(super) rgmii_drive_high: u8,
    pub(super) rgmii_rxc_drive: u8,
    pub(super) rx_delay: u8,
    pub(super) tx_delay_fe: u8,
    pub(super) tx_delay: u8,
    pub(super) tx_inverted_10: bool,
    pub(super) tx_inverted_100: bool,
    pub(super) tx_inverted_1000: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GmacFwConfig {
    pub(super) mmio: (PhysAddr, usize),
    pub(super) mac: [u8; 6],
    pub(super) rx_fifo_depth: usize,
    pub(super) tx_fifo_depth: usize,
    pub(super) rx_pbl: u8,
    pub(super) tx_pbl: u8,
    pub(super) fixed_burst: bool,
    pub(super) axi_write_requests: u8,
    pub(super) axi_read_requests: u8,
    pub(super) axi_burst_map: u8,
    pub(super) force_thresh_dma_mode: bool,
    pub(super) phy: GmacPhyConfig,
}

impl GmacFwConfig {
    pub(super) fn parse(device: &PlatformDevice) -> Result<Self, SysError> {
        let fwnode = device.fwnode().ok_or(SysError::MissingFwNode)?;
        let phy = parse_phy(fwnode.as_ref())?;
        Self::parse_parts(fwnode.as_ref(), device.resources(), phy)
    }

    fn parse_parts(
        fwnode: &dyn FwNode,
        resources: &[Resource],
        phy: GmacPhyConfig,
    ) -> Result<Self, SysError> {
        let mmio = single_mmio_resource(resources).ok_or(SysError::MissingResource)?;
        if fwnode.prop_read_str("phy-mode").as_deref() != Some("rgmii-id") {
            return Err(SysError::DriverIncompatible);
        }
        let mac = parse_mac(fwnode).ok_or(SysError::DriverIncompatible)?;
        let rx_fifo_depth = parse_fifo_depth(fwnode, "rx-fifo-depth", 0x3ff)?;
        let tx_fifo_depth = parse_fifo_depth(fwnode, "tx-fifo-depth", 0x1ff)?;
        let rx_pbl = parse_pbl(fwnode, "snps,rxpbl")?;
        let tx_pbl = parse_pbl(fwnode, "snps,txpbl")?;
        let fixed_burst = fwnode.prop_read_present("snps,fixed-burst");
        let axi_write_requests = parse_axi_requests(fwnode, "snps,write-requests")?;
        let axi_read_requests = parse_axi_requests(fwnode, "snps,read-requests")?;
        let axi_burst_map = parse_axi_burst_map(fwnode)?;
        let force_thresh_dma_mode = fwnode.prop_read_present("snps,force_thresh_dma_mode");
        Ok(Self {
            mmio,
            mac,
            rx_fifo_depth,
            tx_fifo_depth,
            rx_pbl,
            tx_pbl,
            fixed_burst,
            axi_write_requests,
            axi_read_requests,
            axi_burst_map,
            force_thresh_dma_mode,
            phy,
        })
    }
}

fn parse_phy(fwnode: &dyn FwNode) -> Result<GmacPhyConfig, SysError> {
    let node = fwnode.as_of_node().ok_or(SysError::FwNodeLookupFailed)?;
    let mut candidates = node
        .node()
        .children()
        .filter(|child| child.name() == "ethernet-phy");
    let phy = candidates.next().ok_or(SysError::MissingFwNode)?;
    if candidates.next().is_some() {
        return Err(SysError::DriverIncompatible);
    }
    let address = match phy
        .property("reg")
        .and_then(|property| property.value_as_u32())
    {
        Some(address) => address,
        None => u32::from_str_radix(phy.unit_addr().ok_or(SysError::DriverIncompatible)?, 16)
            .map_err(|_| SysError::DriverIncompatible)?,
    };
    let address = u8::try_from(address).map_err(|_| SysError::DriverIncompatible)?;
    if address > 0x1f {
        return Err(SysError::DriverIncompatible);
    }

    let read = |name: &str| {
        phy.property(name)
            .and_then(|property| property.value_as_u32())
    };
    let field = |name: &str, default: u8, max: u32| -> Result<u8, SysError> {
        let value = read(name).unwrap_or(default as u32);
        (value <= max)
            .then_some(value as u8)
            .ok_or(SysError::DriverIncompatible)
    };
    let flag = |name: &str| -> Result<bool, SysError> {
        let value = read(name).unwrap_or(0);
        (value <= 1)
            .then_some(value != 0)
            .ok_or(SysError::DriverIncompatible)
    };

    Ok(GmacPhyConfig {
        address,
        rxc_delay_enable: read("rxc_dly_en").map(|value| value != 0),
        rgmii_drive: field("rgmii_sw_dr", 3, 0x3)?,
        rgmii_drive_high: field("rgmii_sw_dr_2", 0, 0x1)?,
        rgmii_rxc_drive: field("rgmii_sw_dr_rxc", 3, 0x7)?,
        rx_delay: field("rx_delay_sel", 0, 0xf)?,
        tx_delay_fe: field("tx_delay_sel_fe", 0xf, 0xf)?,
        tx_delay: field("tx_delay_sel", 1, 0xf)?,
        tx_inverted_10: flag("tx_inverted_10")?,
        tx_inverted_100: flag("tx_inverted_100")?,
        tx_inverted_1000: flag("tx_inverted_1000")?,
    })
}

fn parse_fifo_depth(
    fwnode: &dyn FwNode,
    property: &str,
    encoded_max: usize,
) -> Result<usize, SysError> {
    let bytes = fwnode
        .prop_read_u32(property)
        .ok_or(SysError::DriverIncompatible)? as usize;
    if bytes < 256 || !bytes.is_multiple_of(256) || bytes / 256 - 1 > encoded_max {
        return Err(SysError::DriverIncompatible);
    }
    Ok(bytes)
}

fn parse_pbl(fwnode: &dyn FwNode, property: &str) -> Result<u8, SysError> {
    let pbl = fwnode
        .prop_read_u32(property)
        .ok_or(SysError::DriverIncompatible)?;
    if !(1..=0x3f).contains(&pbl) {
        return Err(SysError::DriverIncompatible);
    }
    Ok(pbl as u8)
}

fn parse_axi_requests(fwnode: &dyn FwNode, property: &str) -> Result<u8, SysError> {
    // The DT value is the request count; DWMAC stores count - 1 in its 4-bit
    // outstanding-request field. Missing properties retain the reset value 1.
    let requests = fwnode.prop_read_u32(property).unwrap_or(1);
    if !(1..=16).contains(&requests) {
        return Err(SysError::DriverIncompatible);
    }
    Ok((requests - 1) as u8)
}

fn parse_axi_burst_map(fwnode: &dyn FwNode) -> Result<u8, SysError> {
    // Bits 0..=6 select AXI bursts 4..=256 bytes. An absent property leaves
    // the hardware's all-burst default represented explicitly.
    let burst_map = fwnode.prop_read_u32("snps,burst-map").unwrap_or(0x7f);
    if burst_map > 0x7f {
        return Err(SysError::DriverIncompatible);
    }
    Ok(burst_map as u8)
}

fn single_mmio_resource(resources: &[Resource]) -> Option<(PhysAddr, usize)> {
    let mut selected = None;
    for resource in resources {
        let Resource::Mmio { base, len } = resource;
        if selected.is_some() || *len == 0 {
            return None;
        }
        selected = Some((*base, *len));
    }
    selected
}

fn parse_mac(fwnode: &dyn FwNode) -> Option<[u8; 6]> {
    let mac = fwnode.prop_read_raw("local-mac-address")?.try_into().ok()?;
    valid_mac(mac).then_some(mac)
}

fn valid_mac(mac: [u8; 6]) -> bool {
    mac.iter().any(|byte| *byte != 0) && mac[0] & 1 == 0
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::device::discovery::fwnode::{FwNode, StdoutConfig};

    #[derive(Debug)]
    struct FakeFwNode {
        phy_mode: Option<&'static str>,
        local_mac_address: Option<Vec<u8>>,
        rx_fifo_depth: Option<u32>,
        tx_fifo_depth: Option<u32>,
        rx_pbl: Option<u32>,
        tx_pbl: Option<u32>,
        fixed_burst: bool,
        write_requests: Option<u32>,
        read_requests: Option<u32>,
        burst_map: Option<u32>,
        force_thresh_dma_mode: bool,
    }

    impl FakeFwNode {
        fn new(phy_mode: Option<&'static str>, local_mac_address: Option<Vec<u8>>) -> Self {
            Self {
                phy_mode,
                local_mac_address,
                rx_fifo_depth: Some(0x4_0000),
                tx_fifo_depth: Some(0x2_0000),
                rx_pbl: Some(16),
                tx_pbl: Some(16),
                fixed_burst: true,
                write_requests: Some(2),
                read_requests: Some(16),
                burst_map: Some(0x07),
                force_thresh_dma_mode: true,
            }
        }
    }

    impl FwNode for FakeFwNode {
        fn equals(&self, _other: &dyn FwNode) -> bool {
            false
        }

        fn prop_read_u32(&self, prop_name: &str) -> Option<u32> {
            match prop_name {
                "rx-fifo-depth" => self.rx_fifo_depth,
                "tx-fifo-depth" => self.tx_fifo_depth,
                "snps,rxpbl" => self.rx_pbl,
                "snps,txpbl" => self.tx_pbl,
                "snps,write-requests" => self.write_requests,
                "snps,read-requests" => self.read_requests,
                "snps,burst-map" => self.burst_map,
                _ => None,
            }
        }
        fn prop_read_u64(&self, _prop_name: &str) -> Option<u64> {
            None
        }

        fn prop_read_str(&self, prop_name: &str) -> Option<String> {
            (prop_name == "phy-mode")
                .then(|| self.phy_mode.map(ToString::to_string))
                .flatten()
        }

        fn prop_read_present(&self, prop_name: &str) -> bool {
            match prop_name {
                "snps,fixed-burst" => self.fixed_burst,
                "snps,force_thresh_dma_mode" => self.force_thresh_dma_mode,
                _ => self.prop_read_raw(prop_name).is_some(),
            }
        }

        fn prop_read_raw(&self, prop_name: &str) -> Option<&[u8]> {
            match prop_name {
                "local-mac-address" => self
                    .local_mac_address
                    .as_ref()
                    .map(|value| value.as_slice()),
                _ => None,
            }
        }

        fn interrupt_parent(&self) -> Option<Arc<dyn FwNode>> {
            None
        }
        fn interrupt_info(&self) -> Option<&[u8]> {
            None
        }
        fn stdout_config(&self) -> Option<StdoutConfig<'_>> {
            None
        }
    }

    #[cfg(feature = "kunit")]
    fn test_phy() -> GmacPhyConfig {
        GmacPhyConfig {
            address: 0,
            rxc_delay_enable: Some(false),
            rgmii_drive: 3,
            rgmii_drive_high: 0,
            rgmii_rxc_drive: 3,
            rx_delay: 2,
            tx_delay_fe: 5,
            tx_delay: 0,
            tx_inverted_10: true,
            tx_inverted_100: true,
            tx_inverted_1000: false,
        }
    }

    #[cfg(feature = "kunit")]
    fn parse_test_parts(
        node: &dyn FwNode,
        resources: &[Resource],
    ) -> Result<GmacFwConfig, SysError> {
        GmacFwConfig::parse_parts(node, resources, test_phy())
    }

    #[kunit]
    fn local_mac_source_is_required_and_rejects_invalid_values() {
        let local = [0x02, 0, 0, 0, 0, 2];
        let node = FakeFwNode::new(Some("rgmii-id"), Some(local.to_vec()));
        let config = parse_test_parts(
            &node,
            &[Resource::mmio(PhysAddr::new(0x1603_0000), 0x10000)],
        )
        .unwrap();
        assert_eq!(config.mac, local);
        assert_eq!(config.rx_fifo_depth, 0x4_0000);
        assert_eq!(config.tx_fifo_depth, 0x2_0000);
        assert_eq!(config.rx_pbl, 16);
        assert_eq!(config.tx_pbl, 16);
        assert!(config.fixed_burst);
        assert_eq!(config.axi_write_requests, 1);
        assert_eq!(config.axi_read_requests, 15);
        assert_eq!(config.axi_burst_map, 0x07);
        assert!(config.force_thresh_dma_mode);

        let invalid = [0xff; 6];
        let node = FakeFwNode::new(Some("rgmii-id"), Some(invalid.to_vec()));
        assert!(
            parse_test_parts(
                &node,
                &[Resource::mmio(PhysAddr::new(0x1604_0000), 0x10000)],
            )
            .is_err()
        );
    }

    #[kunit]
    fn missing_zero_and_wrong_length_local_mac_fail_closed() {
        for local_mac_address in [None, Some(vec![0; 6]), Some(vec![0x02; 5])] {
            let node = FakeFwNode::new(Some("rgmii-id"), local_mac_address);
            assert!(
                parse_test_parts(
                    &node,
                    &[Resource::mmio(PhysAddr::new(0x1603_0000), 0x10000)]
                )
                .is_err()
            );
        }
    }

    #[kunit]
    fn missing_or_invalid_fifo_and_pbl_facts_fail_closed() {
        let resources = [Resource::mmio(PhysAddr::new(0x1603_0000), 0x10000)];
        let mut node = FakeFwNode::new(Some("rgmii-id"), Some(vec![0x02, 0, 0, 0, 0, 1]));
        node.rx_fifo_depth = None;
        assert!(parse_test_parts(&node, &resources).is_err());

        node.rx_fifo_depth = Some(257);
        assert!(parse_test_parts(&node, &resources).is_err());

        node.rx_fifo_depth = Some(0x4_0000);
        node.tx_fifo_depth = Some(0x2_0100);
        assert!(parse_test_parts(&node, &resources).is_err());

        node.tx_fifo_depth = Some(0x2_0000);
        node.rx_pbl = Some(0);
        assert!(parse_test_parts(&node, &resources).is_err());

        node.rx_pbl = Some(16);
        node.tx_pbl = Some(64);
        assert!(parse_test_parts(&node, &resources).is_err());

        node.tx_pbl = Some(16);
        node.write_requests = Some(0);
        assert!(parse_test_parts(&node, &resources).is_err());

        node.write_requests = Some(2);
        node.read_requests = Some(17);
        assert!(parse_test_parts(&node, &resources).is_err());

        node.read_requests = Some(16);
        node.burst_map = Some(0x80);
        assert!(parse_test_parts(&node, &resources).is_err());
    }
}
