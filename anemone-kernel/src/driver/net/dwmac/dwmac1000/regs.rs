use core::sync::atomic::AtomicU32;

use crate::{mm::remap::IoRemap, prelude::*, time::MonotonicInstant};

use super::{
    fwnode::{DmaOperationMode, LegacyAxiConfig, LegacyDmaConfig, MdioClockRange},
    phy::PhyLink,
};

const DMA_BUS_MODE: usize = 0x1000;
const DMA_TX_POLL_DEMAND: usize = 0x1004;
const DMA_RX_POLL_DEMAND: usize = 0x1008;
const DMA_RX_BASE_ADDR: usize = 0x100c;
const DMA_TX_BASE_ADDR: usize = 0x1010;
const DMA_STATUS: usize = 0x1014;
const DMA_CONTROL: usize = 0x1018;
const DMA_INTERRUPT_ENABLE: usize = 0x101c;
const DMA_RX_WATCHDOG: usize = 0x1024;
const DMA_AXI_BUS_MODE: usize = 0x1028;
const MAC_CONTROL: usize = 0x0000;
const MAC_FRAME_FILTER: usize = 0x0004;
const MAC_HASH_HIGH: usize = 0x0008;
const MAC_HASH_LOW: usize = 0x000c;
const MAC_MII_ADDR: usize = 0x0010;
const MAC_MII_DATA: usize = 0x0014;
const MAC_FLOW_CONTROL: usize = 0x0018;
const MAC_VLAN_TAG: usize = 0x001c;
const MAC_VERSION: usize = 0x0020;
const MAC_PMT: usize = 0x002c;
const MAC_LPI_CONTROL_STATUS: usize = 0x0030;
const MAC_INTERRUPT_STATUS: usize = 0x0038;
const MAC_INTERRUPT_MASK: usize = 0x003c;
const MAC_AN_CONTROL: usize = 0x00c0;
const MAC_AN_STATUS: usize = 0x00c4;
const MAC_RGSMII_STATUS: usize = 0x00d8;
const DMA_HW_FEATURE: usize = 0x1058;
const MAC_ADDR_HIGH: usize = 0x0040;
const MAC_ADDR_LOW: usize = 0x0044;
const MMC_RX_INTERRUPT: usize = 0x0104;
const MMC_CONTROL: usize = 0x0100;
const MMC_TX_INTERRUPT: usize = 0x0108;
const MMC_RX_INTERRUPT_MASK: usize = 0x010c;
const MMC_TX_INTERRUPT_MASK: usize = 0x0110;
const MMC_RX_IPC_INTERRUPT_MASK: usize = 0x0200;
const MMC_RX_IPC_INTERRUPT: usize = 0x0208;
const PTP_TIMESTAMP_CONTROL: usize = 0x0700;
const EXPECTED_CORE_VERSION: u8 = 0x37;
const SW_RESET: u32 = 1;
const ALTERNATE_DESCRIPTOR_SIZE: u32 = 1 << 7;
const MII_BUSY: u32 = 1;
const MII_CLOCK_RANGE_SHIFT: u32 = 2;
const MII_CLOCK_RANGE_MASK: u32 = 0xf << MII_CLOCK_RANGE_SHIFT;
const HW_MII: u32 = 1 << 0;
const HW_GMII: u32 = 1 << 1;
const HW_HALF_DUPLEX: u32 = 1 << 2;
const HW_PCS: u32 = 1 << 6;
const HW_MDIO: u32 = 1 << 8;
const HW_REMOTE_WAKE: u32 = 1 << 9;
const HW_MAGIC_WAKE: u32 = 1 << 10;
const HW_MMC: u32 = 1 << 11;
const HW_TIMESTAMP_V1: u32 = 1 << 12;
const HW_TIMESTAMP_V2: u32 = 1 << 13;
const HW_EEE: u32 = 1 << 14;
const HW_TX_CHECKSUM: u32 = 1 << 16;
const HW_RX_CHECKSUM_TYPE1: u32 = 1 << 17;
const HW_RX_CHECKSUM_TYPE2: u32 = 1 << 18;
const HW_RX_FIFO_OVER_2048: u32 = 1 << 19;
const HW_RX_CHANNEL_MASK: u32 = 0x3 << 20;
const HW_TX_CHANNEL_MASK: u32 = 0x3 << 22;
const ENHDESSEL: u32 = 1 << 24;
const MAC_LOOPBACK: u32 = 1 << 12;
const MAC_FULL_DUPLEX: u32 = 1 << 11;
const MAC_TX_ENABLE: u32 = 1 << 3;
const MAC_RX_ENABLE: u32 = 1 << 2;
const DMA_TX_START: u32 = 1 << 13;
const DMA_RX_START: u32 = 1 << 1;
const DMA_TX_STORE_FORWARD: u32 = 1 << 21;
const DMA_RX_STORE_FORWARD: u32 = 1 << 25;
const DMA_TX_THRESHOLD_MASK: u32 = 0x7 << 14;
const DMA_RX_THRESHOLD_MASK: u32 = 0x3 << 3;
const DMA_OPERATE_SECOND_FRAME: u32 = 1 << 2;
const DMA_FLOW_CONTROL_ENABLE: u32 = 1 << 8;
const DMA_FLOW_ACTIVATION_MASK: u32 = 0x0080_0600;
const DMA_FLOW_DEACTIVATION_MASK: u32 = 0x0040_1800;
const DMA_TX_PROCESS_MASK: u32 = 0x0070_0000;
const DMA_TX_PROCESS_SHIFT: u32 = 20;
const DMA_RX_PROCESS_MASK: u32 = 0x000e_0000;
const DMA_RX_PROCESS_SHIFT: u32 = 17;
const DMA_PBL_MASK: u32 = 0x3f << 8;
const DMA_PBL_SHIFT: u32 = 8;
const DMA_RX_PBL_MASK: u32 = 0x3f << 17;
const DMA_RX_PBL_SHIFT: u32 = 17;
const DMA_USE_SEPARATE_PBL: u32 = 1 << 23;
const DMA_PBL_X8: u32 = 1 << 24;
const DMA_ADDRESS_ALIGNED_BEATS: u32 = 1 << 25;
const DMA_MIXED_BURST: u32 = 1 << 26;
const DMA_FIXED_BURST: u32 = 1 << 16;
const DMA_AXI_LPI_ENABLE: u32 = 1 << 31;
const DMA_AXI_EXIT_FRAME: u32 = 1 << 30;
const DMA_AXI_WRITE_LIMIT_MASK: u32 = 0xf << 20;
const DMA_AXI_READ_LIMIT_MASK: u32 = 0xf << 16;
const MAC_JABBER_DISABLE: u32 = 1 << 22;
const MAC_FRAME_BURST: u32 = 1 << 21;
const MAC_JUMBO_ENABLE: u32 = 1 << 20;
const MAC_2K_ENABLE: u32 = 1 << 27;
const MAC_WATCHDOG_DISABLE: u32 = 1 << 23;
const MAC_DISABLE_CARRIER_SENSE: u32 = 1 << 16;
const MAC_PORT_SELECT: u32 = 1 << 15;
const MAC_FAST_ETHERNET_SPEED: u32 = 1 << 14;
const MAC_AUTO_PAD_FCS_STRIP: u32 = 1 << 7;
const MAC_RX_CHECKSUM: u32 = 1 << 10;
const MAC_LINK_MASK: u32 = MAC_PORT_SELECT | MAC_FAST_ETHERNET_SPEED | MAC_FULL_DUPLEX;
const MAC_FILTER_HASH_OR_PERFECT: u32 = 1 << 10;
const MAC_FLOW_UNICAST_PAUSE: u32 = 1 << 3;
const MAC_FLOW_RX_ENABLE: u32 = 1 << 2;
const MAC_FLOW_TX_ENABLE: u32 = 1 << 1;
const MAC_FLOW_PAUSE_TIME: u32 = 0xffff << 16;
const MAC_PMT_ENABLE_MASK: u32 = (1 << 9) | (1 << 2) | (1 << 1) | 1;
const MAC_LPI_ENABLE_MASK: u32 = (1 << 19) | (1 << 16);
const MAC_AN_ENABLE_RESTART: u32 = (1 << 12) | (1 << 9);
const MAC_INTERRUPT_RGMII: u32 = 1 << 0;
const MAC_INTERRUPT_PCS_LINK: u32 = 1 << 1;
const MAC_INTERRUPT_PCS_AN: u32 = 1 << 2;
const MAC_INTERRUPT_PMT: u32 = 1 << 3;
const MAC_INTERRUPT_MMC_RX: u32 = 1 << 5;
const MAC_INTERRUPT_MMC_TX: u32 = 1 << 6;
const MAC_INTERRUPT_MMC_IPC: u32 = 1 << 7;
const MAC_INTERRUPT_LPI: u32 = 1 << 10;
const MMC_CONTROL_LINUX_INITIAL: u32 = 0x35;
const PHY_ID_YT8511: u32 = 0x0000_010a;
// The probe consumes RGMII status like Linux's PCS path, while keeping the
// other defined legacy MAC sources masked. Do not write reserved bits.
const MASK_LEGACY_MAC_INTERRUPTS_REQUESTED: u32 = 0x20f;
const MASK_ALL_MMC_INTERRUPTS: u32 = u32::MAX;
const DMA_NORMAL_INTERRUPT: u32 = 1 << 16;
const DMA_ABNORMAL_INTERRUPT: u32 = 1 << 15;
const DMA_RX_INTERRUPT: u32 = 1 << 6;
const DMA_TX_INTERRUPT: u32 = 1;

static_assert!(
    DWMAC1000_RESET_TIMEOUT_MS > 0,
    "DWMAC1000_RESET_TIMEOUT_MS must be greater than zero"
);
static_assert!(
    DWMAC1000_MDIO_TIMEOUT_MS > 0,
    "DWMAC1000_MDIO_TIMEOUT_MS must be greater than zero"
);
static_assert!(
    DWMAC1000_PROBE_TIMEOUT_MS > 0,
    "DWMAC1000_PROBE_TIMEOUT_MS must be greater than zero"
);

#[derive(Debug, Clone, Copy)]
pub(super) struct Dwmac1000Capabilities {
    pub(super) version: u32,
    pub(super) hw_feature: u32,
    pub(super) enhanced_descriptors: bool,
    pub(super) mii: bool,
    pub(super) gmii: bool,
    pub(super) half_duplex: bool,
    pub(super) pcs: bool,
    pub(super) mdio: bool,
    pub(super) remote_wake: bool,
    pub(super) magic_wake: bool,
    pub(super) rmon: bool,
    pub(super) timestamp_v1: bool,
    pub(super) timestamp_v2: bool,
    pub(super) eee: bool,
    pub(super) tx_checksum: bool,
    pub(super) rx_checksum_type1: bool,
    pub(super) rx_checksum_type2: bool,
    pub(super) rx_fifo_over_2048: bool,
    pub(super) rx_channels: u8,
    pub(super) tx_channels: u8,
}

impl Dwmac1000Capabilities {
    pub(super) const fn expected_family(self) -> bool {
        self.version as u8 == EXPECTED_CORE_VERSION
    }

    pub(super) const fn supported_gate2(self) -> bool {
        self.hw_feature != 0
            && (self.mii || self.gmii)
            && self.mdio
            && self.enhanced_descriptors
            && self.rx_channels == 1
            && self.tx_channels == 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SelectedDmaMode {
    ThresholdBoth,
    StoreForwardBoth,
    ThresholdTxStoreForwardRx,
}

impl SelectedDmaMode {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::ThresholdBoth => "threshold-both",
            Self::StoreForwardBoth => "store-forward-both",
            Self::ThresholdTxStoreForwardRx => "threshold-tx-store-forward-rx",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PhySnapshot {
    pub(super) id1: u16,
    pub(super) id2: u16,
    pub(super) bmcr: u16,
    pub(super) bmsr: u16,
}

impl PhySnapshot {
    pub(super) const fn model(self) -> &'static str {
        match ((self.id1 as u32) << 16) | self.id2 as u32 {
            PHY_ID_YT8511 => "Motorcomm-YT8511",
            _ => "unconfirmed",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct DmaResetSnapshot {
    pub(super) before: u32,
    pub(super) after: u32,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct QuiesceSnapshot {
    pub(super) status: u32,
    pub(super) active_legal: u32,
    pub(super) cleanup_legal: u32,
    pub(super) uncleared: u32,
    pub(super) w1c_samples: u32,
    pub(super) control: u32,
    pub(super) mac_control: u32,
    pub(super) tx_process: u32,
    pub(super) rx_process: u32,
    pub(super) stopped: bool,
    pub(super) mac_status_before: u32,
    pub(super) mac_status_after: u32,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ProbeStartSnapshot {
    pub(super) mac_enabled_control: u32,
    pub(super) rx_started_control: u32,
    pub(super) tx_started_control: u32,
    pub(super) hash_high: u32,
    pub(super) hash_low: u32,
    pub(super) frame_filter: u32,
    pub(super) loopback_control: u32,
    pub(super) flow_control: u32,
    pub(super) interrupt_enable: u32,
}

impl ProbeStartSnapshot {
    pub(super) const fn linux_sequence_valid(self) -> bool {
        self.mac_enabled_control & (MAC_TX_ENABLE | MAC_RX_ENABLE) == MAC_TX_ENABLE | MAC_RX_ENABLE
            && self.mac_enabled_control & MAC_LOOPBACK == 0
            && self.rx_started_control & (DMA_RX_START | DMA_TX_START) == DMA_RX_START
            && self.tx_started_control & (DMA_RX_START | DMA_TX_START)
                == DMA_RX_START | DMA_TX_START
            && self.hash_high == 0
            && self.hash_low == 0
            && self.frame_filter == MAC_FILTER_HASH_OR_PERFECT
            && self.loopback_control & (MAC_TX_ENABLE | MAC_RX_ENABLE | MAC_LOOPBACK)
                == MAC_TX_ENABLE | MAC_RX_ENABLE | MAC_LOOPBACK
            && self.interrupt_enable == 0
    }
}

pub(super) struct Dwmac1000Regs {
    remap: IoRemap,
    mdio_clock_range: AtomicU32,
}

impl Dwmac1000Regs {
    const REQUIRED_MAPPING_LEN: usize = DMA_HW_FEATURE + 4;

    pub(super) fn new(remap: IoRemap) -> Result<Self, SysError> {
        if remap.size() < Self::REQUIRED_MAPPING_LEN as u64 {
            return Err(SysError::DriverIncompatible);
        }
        Ok(Self {
            remap,
            mdio_clock_range: AtomicU32::new(0),
        })
    }

    fn ptr_at(&self, offset: usize) -> *mut u32 {
        let end = offset.checked_add(core::mem::size_of::<u32>()).unwrap();
        assert!(end <= self.remap.size() as usize);
        unsafe { self.remap.as_ptr().as_ptr().cast::<u8>().add(offset).cast() }
    }

    fn read(&self, offset: usize) -> u32 {
        core::sync::atomic::fence(Ordering::SeqCst);
        let value = unsafe { core::ptr::read_volatile(self.ptr_at(offset)) };
        core::sync::atomic::fence(Ordering::SeqCst);
        value
    }

    fn write(&self, offset: usize, value: u32) {
        core::sync::atomic::fence(Ordering::SeqCst);
        unsafe { core::ptr::write_volatile(self.ptr_at(offset), value) }
    }

    pub(super) fn capabilities(&self) -> Dwmac1000Capabilities {
        let version = self.read(MAC_VERSION);
        let hw_feature = self.read(DMA_HW_FEATURE);
        decode_capabilities(version, hw_feature)
    }

    pub(super) fn reset_dma(&self) -> Result<DmaResetSnapshot, DmaResetSnapshot> {
        let before = self.read(DMA_BUS_MODE);
        let mut mode = before;
        mode |= SW_RESET;
        self.write(DMA_BUS_MODE, mode);
        let start = MonotonicInstant::now();
        loop {
            let current = self.read(DMA_BUS_MODE);
            if current & SW_RESET == 0 {
                return Ok(DmaResetSnapshot {
                    before,
                    after: current,
                });
            }
            if start.elapsed() >= Duration::from_millis(DWMAC1000_RESET_TIMEOUT_MS) {
                return Err(DmaResetSnapshot {
                    before,
                    after: current,
                });
            }
            core::hint::spin_loop();
        }
    }

    pub(super) const fn atds(bus_mode: u32) -> bool {
        bus_mode & ALTERNATE_DESCRIPTOR_SIZE != 0
    }

    pub(super) fn mdio_clock_range(&self) -> Option<MdioClockRange> {
        MdioClockRange::new(self.mdio_clock_range_raw() as u32)
    }

    pub(super) fn mdio_clock_range_raw(&self) -> u8 {
        ((self.read(MAC_MII_ADDR) & MII_CLOCK_RANGE_MASK) >> MII_CLOCK_RANGE_SHIFT) as u8
    }

    pub(super) fn set_mdio_clock_range(&self, value: MdioClockRange) {
        self.mdio_clock_range.store(
            (value.encoded() as u32) << MII_CLOCK_RANGE_SHIFT,
            Ordering::Release,
        );
    }

    pub(super) fn mdio_divider(&self) -> u32 {
        self.mdio_clock_range.load(Ordering::Acquire) >> MII_CLOCK_RANGE_SHIFT
    }

    pub(super) fn phy_snapshot(&self, phy: u8) -> Result<PhySnapshot, SysError> {
        Ok(PhySnapshot {
            id1: self.mdio_read(phy, 2)?,
            id2: self.mdio_read(phy, 3)?,
            bmcr: self.mdio_read(phy, 0)?,
            bmsr: self.mdio_read(phy, 1)?,
        })
    }

    pub(super) fn mdio_read(&self, phy: u8, register: u8) -> Result<u16, SysError> {
        self.wait_mdio_idle()?;
        self.write(MAC_MII_ADDR, self.mdio_command(phy, register, false));
        self.wait_mdio_idle()?;
        Ok(self.read(MAC_MII_DATA) as u16)
    }

    pub(super) fn mdio_write(&self, phy: u8, register: u8, value: u16) -> Result<(), SysError> {
        self.wait_mdio_idle()?;
        self.write(MAC_MII_DATA, value as u32);
        self.write(MAC_MII_ADDR, self.mdio_command(phy, register, true));
        self.wait_mdio_idle()
    }

    fn wait_mdio_idle(&self) -> Result<(), SysError> {
        let start = MonotonicInstant::now();
        loop {
            if self.read(MAC_MII_ADDR) & MII_BUSY == 0 {
                return Ok(());
            }
            if start.elapsed() >= Duration::from_millis(DWMAC1000_MDIO_TIMEOUT_MS) {
                return Err(SysError::Timeout);
            }
            core::hint::spin_loop();
        }
    }

    pub(super) fn mdio_address(&self) -> u32 {
        self.read(MAC_MII_ADDR)
    }

    pub(super) fn prepare_probe(
        &self,
        mac: [u8; 6],
        rx_desc: u32,
        tx_desc: u32,
        dma: LegacyDmaConfig,
        axi: Option<LegacyAxiConfig>,
        pcs_initial_speed: Option<u32>,
        capabilities: Dwmac1000Capabilities,
        link: PhyLink,
    ) -> Result<ProbeRegisterSnapshot, SysError> {
        self.write(DMA_INTERRUPT_ENABLE, 0);
        self.write(MAC_INTERRUPT_MASK, MASK_LEGACY_MAC_INTERRUPTS_REQUESTED);
        self.mask_mmc_interrupts();
        let mut bus_mode = self.read(DMA_BUS_MODE);
        bus_mode &= !(ALTERNATE_DESCRIPTOR_SIZE
            | DMA_PBL_MASK
            | DMA_RX_PBL_MASK
            | DMA_USE_SEPARATE_PBL
            | DMA_PBL_X8
            | DMA_FIXED_BURST
            | DMA_MIXED_BURST
            | DMA_ADDRESS_ALIGNED_BEATS);
        bus_mode |= ALTERNATE_DESCRIPTOR_SIZE
            | DMA_USE_SEPARATE_PBL
            | (dma.tx_pbl.encoded() as u32) << DMA_PBL_SHIFT
            | (dma.rx_pbl.encoded() as u32) << DMA_RX_PBL_SHIFT;
        if dma.pbl_x8 {
            bus_mode |= DMA_PBL_X8;
        }
        if dma.fixed_burst {
            bus_mode |= DMA_FIXED_BURST;
        }
        if dma.mixed_burst {
            bus_mode |= DMA_MIXED_BURST;
        }
        if dma.address_aligned_beats {
            bus_mode |= DMA_ADDRESS_ALIGNED_BEATS;
        }
        self.write(DMA_BUS_MODE, bus_mode);
        let axi_bus_mode = axi.map(|axi| self.initialize_axi(axi));

        let selected_dma_mode = select_dma_mode(dma.operation_mode, capabilities.tx_checksum);
        let mut dma_control = self.read(DMA_CONTROL);
        dma_control &= !(DMA_TX_START
            | DMA_RX_START
            | DMA_TX_STORE_FORWARD
            | DMA_RX_STORE_FORWARD
            | DMA_OPERATE_SECOND_FRAME
            | DMA_TX_THRESHOLD_MASK
            | DMA_RX_THRESHOLD_MASK
            | DMA_FLOW_CONTROL_ENABLE
            | DMA_FLOW_ACTIVATION_MASK
            | DMA_FLOW_DEACTIVATION_MASK);
        dma_control |= dma_mode_bits(selected_dma_mode);
        dma_control = configure_fifo_flow_control(dma_control, dma.rx_fifo_bytes);
        self.write(DMA_CONTROL, dma_control);

        let mut mac_control = self.read(MAC_CONTROL);
        mac_control &= !(MAC_TX_ENABLE
            | MAC_RX_ENABLE
            | MAC_LOOPBACK
            | MAC_LINK_MASK
            | MAC_AUTO_PAD_FCS_STRIP
            | MAC_RX_CHECKSUM
            | MAC_JUMBO_ENABLE
            | MAC_2K_ENABLE
            | MAC_WATCHDOG_DISABLE);
        let pcs_initial_control = if capabilities.pcs {
            if let Some(speed) = pcs_initial_speed {
                mac_control |= mac_link_bits(speed, false)?;
                self.write(MAC_CONTROL, mac_control);
                Some(self.read(MAC_CONTROL))
            } else {
                None
            }
        } else {
            None
        };
        mac_control &= !MAC_LINK_MASK;
        // Linux programs PS/FES/DM from the resolved PHY link before enabling
        // the MAC. Internal loopback still consumes that MAC clock selection.
        mac_control |= MAC_JABBER_DISABLE
            | MAC_FRAME_BURST
            | MAC_DISABLE_CARRIER_SENSE
            | mac_link_bits(link.speed_mbps, link.full_duplex)?;
        self.write(MAC_CONTROL, mac_control);
        self.write(MAC_FLOW_CONTROL, mac_flow_control(link));
        self.write(DMA_RX_BASE_ADDR, rx_desc);
        self.write(DMA_TX_BASE_ADDR, tx_desc);
        self.write(
            MAC_ADDR_HIGH,
            ((mac[5] as u32) << 8) | mac[4] as u32 | (1 << 31),
        );
        self.write(
            MAC_ADDR_LOW,
            ((mac[3] as u32) << 24)
                | ((mac[2] as u32) << 16)
                | ((mac[1] as u32) << 8)
                | mac[0] as u32,
        );
        // Empty address lists use address 0 perfect matching. Program this
        // before any RX path can run so loopback traffic cannot observe the
        // firmware filter state.
        self.write(MAC_HASH_HIGH, 0);
        self.write(MAC_HASH_LOW, 0);
        self.write(MAC_FRAME_FILTER, MAC_FILTER_HASH_OR_PERFECT);

        self.write(MAC_VLAN_TAG, 0);
        let has_pmt = capabilities.remote_wake || capabilities.magic_wake;
        let pmt = if has_pmt {
            self.write(MAC_PMT, 0);
            self.read(MAC_PMT)
        } else {
            0
        };
        let lpi_control_status = if capabilities.eee {
            let value = self.read(MAC_LPI_CONTROL_STATUS) & !MAC_LPI_ENABLE_MASK;
            self.write(MAC_LPI_CONTROL_STATUS, value);
            self.read(MAC_LPI_CONTROL_STATUS)
        } else {
            0
        };
        let has_timestamp = capabilities.timestamp_v1 || capabilities.timestamp_v2;
        let timestamp_control = if has_timestamp {
            self.write(PTP_TIMESTAMP_CONTROL, 0);
            self.read(PTP_TIMESTAMP_CONTROL)
        } else {
            0
        };
        self.initialize_mmc(capabilities.rmon);

        let pcs_selected = capabilities.pcs;
        let (pcs_an_control, pcs_an_status) = if pcs_selected {
            let mut an_control = self.read(MAC_AN_CONTROL);
            an_control |= MAC_AN_ENABLE_RESTART;
            self.write(MAC_AN_CONTROL, an_control);
            (self.read(MAC_AN_CONTROL), self.read(MAC_AN_STATUS))
        } else {
            (0, 0)
        };
        self.acknowledge_causes(self.status() & super::protocol::CSR5_W1C_MASK);

        // Linux writes all mask bits. Legacy DWMAC1000 exposes only the
        // implemented counter fields on readback, so an effective mask is
        // diagnostic evidence rather than an exact u32::MAX admission value.
        let snapshot = ProbeRegisterSnapshot {
            bus_mode: self.read(DMA_BUS_MODE),
            axi_bus_mode,
            rx_desc: self.read(DMA_RX_BASE_ADDR),
            tx_desc: self.read(DMA_TX_BASE_ADDR),
            mac_high: self.read(MAC_ADDR_HIGH),
            mac_low: self.read(MAC_ADDR_LOW),
            dma_control: self.read(DMA_CONTROL),
            mac_control: self.read(MAC_CONTROL),
            flow_control: self.read(MAC_FLOW_CONTROL),
            interrupt_enable: self.interrupt_enable(),
            mac_interrupt_mask: self.read(MAC_INTERRUPT_MASK),
            mmc_rx_interrupt_mask: self.read(MMC_RX_INTERRUPT_MASK),
            mmc_tx_interrupt_mask: self.read(MMC_TX_INTERRUPT_MASK),
            mmc_rx_ipc_interrupt_mask: self.read(MMC_RX_IPC_INTERRUPT_MASK),
            mmc_control: if capabilities.rmon {
                self.read(MMC_CONTROL)
            } else {
                0
            },
            hash_high: self.read(MAC_HASH_HIGH),
            hash_low: self.read(MAC_HASH_LOW),
            frame_filter: self.read(MAC_FRAME_FILTER),
            vlan_tag: self.read(MAC_VLAN_TAG),
            pmt,
            lpi_control_status,
            timestamp_control,
            rx_watchdog: self.read(DMA_RX_WATCHDOG),
            pcs_selected,
            pcs_initial_speed,
            pcs_initial_control,
            pcs_an_control,
            pcs_an_status,
            selected_dma_mode,
            status: self.status(),
        };
        let expected_mac_low = ((mac[3] as u32) << 24)
            | ((mac[2] as u32) << 16)
            | ((mac[1] as u32) << 8)
            | mac[0] as u32;
        let expected_mac_high = ((mac[5] as u32) << 8) | mac[4] as u32;
        let mut mismatch = 0u32;
        if !Self::atds(snapshot.bus_mode) {
            mismatch |= 1 << 0;
        }
        if snapshot.rx_desc != rx_desc {
            mismatch |= 1 << 1;
        }
        if snapshot.tx_desc != tx_desc {
            mismatch |= 1 << 2;
        }
        if snapshot.mac_high & 0xffff != expected_mac_high {
            mismatch |= 1 << 3;
        }
        if snapshot.mac_low != expected_mac_low {
            mismatch |= 1 << 4;
        }
        if snapshot.dma_control & (DMA_TX_START | DMA_RX_START) != 0 {
            mismatch |= 1 << 5;
        }
        if snapshot.interrupt_enable != 0 {
            mismatch |= 1 << 6;
        }
        if snapshot.mac_control
            & (MAC_TX_ENABLE
                | MAC_RX_ENABLE
                | MAC_LOOPBACK
                | MAC_AUTO_PAD_FCS_STRIP
                | MAC_RX_CHECKSUM)
            != 0
        {
            mismatch |= 1 << 7;
        }
        if snapshot.mac_control & MAC_LINK_MASK != mac_link_bits(link.speed_mbps, link.full_duplex)?
        {
            mismatch |= 1 << 8;
        }
        if snapshot.hash_high != 0
            || snapshot.hash_low != 0
            || snapshot.frame_filter != MAC_FILTER_HASH_OR_PERFECT
        {
            mismatch |= 1 << 9;
        }
        if snapshot.vlan_tag != 0
            || (has_pmt && snapshot.pmt & MAC_PMT_ENABLE_MASK != 0)
            || (capabilities.eee && snapshot.lpi_control_status & MAC_LPI_ENABLE_MASK != 0)
            || (has_timestamp && snapshot.timestamp_control != 0)
        {
            mismatch |= 1 << 10;
        }
        if snapshot.pcs_selected
            && snapshot.pcs_an_control & MAC_AN_ENABLE_RESTART != MAC_AN_ENABLE_RESTART
        {
            mismatch |= 1 << 12;
        }
        if snapshot.dma_control & dma_mode_mask() != dma_mode_bits(selected_dma_mode) {
            mismatch |= 1 << 13;
        }
        if fifo_flow_control_enabled(snapshot.dma_control)
            != fifo_flow_control_expected(dma.rx_fifo_bytes)
        {
            mismatch |= 1 << 14;
        }
        if snapshot.mac_interrupt_mask != effective_mac_interrupt_mask(capabilities) {
            mismatch |= 1 << 15;
        }
        if mismatch != 0 {
            kerrln!(
                "dwmac1000 stage=probe-register-readback result=fail mismatch-mask={:#x} expected-rx-desc={:#x} actual-rx-desc={:#x} expected-tx-desc={:#x} actual-tx-desc={:#x}",
                mismatch,
                rx_desc,
                snapshot.rx_desc,
                tx_desc,
                snapshot.tx_desc,
            );
            kerrln!(
                "dwmac1000 stage=probe-register-readback-detail result=fail expected-mac-high={:#x} actual-mac-high={:#x} expected-mac-low={:#x} actual-mac-low={:#x} bus-mode={:#x} csr6={:#x} mac-control={:#x} csr5={:#x}",
                expected_mac_high,
                snapshot.mac_high,
                expected_mac_low,
                snapshot.mac_low,
                snapshot.bus_mode,
                snapshot.dma_control,
                snapshot.mac_control,
                snapshot.status,
            );
            kerrln!(
                "dwmac1000 stage=probe-register-readback-mmc result=observed rmon={} mmc-control={:#x} mmc-rx-mask-effective={:#x} mmc-tx-mask-effective={:#x} mmc-ipc-mask-effective={:#x} mac-mask-effective={:#x} mac-mask-expected={:#x}",
                capabilities.rmon,
                snapshot.mmc_control,
                snapshot.mmc_rx_interrupt_mask,
                snapshot.mmc_tx_interrupt_mask,
                snapshot.mmc_rx_ipc_interrupt_mask,
                snapshot.mac_interrupt_mask,
                effective_mac_interrupt_mask(capabilities),
            );
            return Err(SysError::ProbeFailed);
        }
        Ok(snapshot)
    }

    pub(super) fn start_probe(&self) -> ProbeStartSnapshot {
        self.acknowledge_causes(self.status() & super::protocol::CSR5_W1C_MASK);
        // Gate 2 runs before the architecture enables local interrupts. Keep
        // every device interrupt source suppressed and observe CSR5 directly;
        // Gate 3 owns the first IRQ request and CSR7 enable transition.
        self.suppress_interrupts();
        let hash_high = self.read(MAC_HASH_HIGH);
        let hash_low = self.read(MAC_HASH_LOW);
        let frame_filter = self.read(MAC_FRAME_FILTER);

        let mut mac = self.read(MAC_CONTROL);
        mac &= !MAC_LOOPBACK;
        mac |= MAC_TX_ENABLE | MAC_RX_ENABLE;
        self.write(MAC_CONTROL, mac);
        let mac_enabled_control = self.read(MAC_CONTROL);

        let mut dma = self.read(DMA_CONTROL);
        // Linux starts the legacy RX channel before the TX channel. Keep the
        // two commits distinct so TX cannot consume a descriptor before the
        // receive ring is live.
        dma |= DMA_RX_START;
        self.write(DMA_CONTROL, dma);
        let rx_started_control = self.read(DMA_CONTROL);
        dma |= DMA_TX_START;
        self.write(DMA_CONTROL, dma);
        let tx_started_control = self.read(DMA_CONTROL);

        mac = self.read(MAC_CONTROL);
        mac |= MAC_LOOPBACK;
        self.write(MAC_CONTROL, mac);
        ProbeStartSnapshot {
            mac_enabled_control,
            rx_started_control,
            tx_started_control,
            hash_high,
            hash_low,
            frame_filter,
            loopback_control: self.read(MAC_CONTROL),
            flow_control: self.read(MAC_FLOW_CONTROL),
            interrupt_enable: self.interrupt_enable(),
        }
    }

    fn mdio_command(&self, phy: u8, register: u8, write: bool) -> u32 {
        mdio_command(
            self.mdio_clock_range.load(Ordering::Acquire),
            phy,
            register,
            write,
        )
    }

    pub(super) fn demand_tx(&self) {
        self.write(DMA_TX_POLL_DEMAND, 1);
    }

    pub(super) fn demand_rx(&self) {
        self.write(DMA_RX_POLL_DEMAND, 1);
    }

    pub(super) fn start_runtime(&self) {
        // Gate 3 adopts the quiesced Gate 2 owner. Clear stale device causes
        // before enabling CSR7 so the first level dispatch represents a new
        // event, then follow Linux's RX-before-TX start order.
        self.service_mac_interrupts();
        self.acknowledge_causes(self.status() & super::protocol::CSR5_W1C_MASK);
        let mut mac = self.read(MAC_CONTROL);
        mac |= MAC_TX_ENABLE | MAC_RX_ENABLE;
        self.write(MAC_CONTROL, mac);
        let mut dma = self.read(DMA_CONTROL);
        dma |= DMA_RX_START;
        self.write(DMA_CONTROL, dma);
        dma |= DMA_TX_START;
        self.write(DMA_CONTROL, dma);
        // The Gate 2 mask is a suppression baseline. Gate 3 restores the
        // Linux host path; read-to-clear MAC/PCS/MMC causes are then drained
        // by the same IRQ handler before level unmask.
        self.write(MAC_INTERRUPT_MASK, 0);
        self.write(
            DMA_INTERRUPT_ENABLE,
            DMA_NORMAL_INTERRUPT | DMA_ABNORMAL_INTERRUPT | DMA_RX_INTERRUPT | DMA_TX_INTERRUPT,
        );
    }

    pub(super) fn status(&self) -> u32 {
        self.read(DMA_STATUS)
    }

    pub(super) fn interrupt_enable(&self) -> u32 {
        self.read(DMA_INTERRUPT_ENABLE)
    }

    /// Read-only runtime controls used by Gate 3 diagnostics. These values
    /// mirror the live MMIO owner and never participate in interrupt or DMA
    /// decisions; remove or reduce the caller after Gate 3 acceptance.
    pub(super) fn runtime_snapshot(&self) -> RuntimeRegisterSnapshot {
        RuntimeRegisterSnapshot {
            status: self.status(),
            dma_control: self.read(DMA_CONTROL),
            interrupt_enable: self.interrupt_enable(),
            mac_control: self.read(MAC_CONTROL),
            mac_interrupt_mask: self.read(MAC_INTERRUPT_MASK),
            mac_address_high: self.read(MAC_ADDR_HIGH),
            mac_address_low: self.read(MAC_ADDR_LOW),
            frame_filter: self.read(MAC_FRAME_FILTER),
        }
    }

    pub(super) fn acknowledge_causes(&self, causes: u32) -> u32 {
        if causes != 0 {
            assert_eq!(causes & !super::protocol::CSR5_W1C_MASK, 0);
            self.write(DMA_STATUS, causes);
        }
        // W1C evidence is only valid when the same owner performs the
        // immediate readback. Callers may use this as their next raw sample.
        self.status()
    }

    fn mask_mmc_interrupts(&self) {
        self.write(MMC_RX_INTERRUPT_MASK, MASK_ALL_MMC_INTERRUPTS);
        self.write(MMC_TX_INTERRUPT_MASK, MASK_ALL_MMC_INTERRUPTS);
        self.write(MMC_RX_IPC_INTERRUPT_MASK, MASK_ALL_MMC_INTERRUPTS);
    }

    fn initialize_mmc(&self, rmon: bool) {
        self.mask_mmc_interrupts();
        if rmon {
            // Linux treats these control bits as an initialization command.
            // COUNTER_RESET/PRESET/FULL_HALF_PRESET may self-clear; only the
            // write is protocol truth, while the post-write value is retained
            // as diagnostic evidence below.
            let control = self.read(MMC_CONTROL) | MMC_CONTROL_LINUX_INITIAL;
            self.write(MMC_CONTROL, control);
        }
    }

    fn initialize_axi(&self, config: LegacyAxiConfig) -> u32 {
        let mut mode = self.read(DMA_AXI_BUS_MODE);
        mode &= !(DMA_AXI_WRITE_LIMIT_MASK | DMA_AXI_READ_LIMIT_MASK);
        mode |= (config.write_outstanding_limit as u32) << 20
            | (config.read_outstanding_limit as u32) << 16
            | config.burst_mask as u32;
        if config.lpi_enable {
            mode |= DMA_AXI_LPI_ENABLE;
        }
        if config.exit_frame {
            mode |= DMA_AXI_EXIT_FRAME;
        }
        self.write(DMA_AXI_BUS_MODE, mode);
        self.read(DMA_AXI_BUS_MODE)
    }

    pub(super) fn suppress_interrupts(&self) {
        self.write(DMA_INTERRUPT_ENABLE, 0);
        self.write(MAC_INTERRUPT_MASK, MASK_LEGACY_MAC_INTERRUPTS_REQUESTED);
        self.mask_mmc_interrupts();
    }

    /// Drain every legacy host cause that Linux services through a
    /// read-to-clear register. Gate 2 keeps these sources masked, but must not
    /// hand stale device state to Gate 3's first unmask.
    pub(super) fn service_mac_interrupts(&self) -> (u32, u32) {
        let status = self.read(MAC_INTERRUPT_STATUS);
        if status & (MAC_INTERRUPT_PCS_LINK | MAC_INTERRUPT_PCS_AN) != 0 {
            self.read(MAC_AN_STATUS);
        }
        if status & MAC_INTERRUPT_RGMII != 0 {
            self.read(MAC_RGSMII_STATUS);
        }
        if status & MAC_INTERRUPT_PMT != 0 {
            self.read(MAC_PMT);
        }
        if status & MAC_INTERRUPT_LPI != 0 {
            self.read(MAC_LPI_CONTROL_STATUS);
        }
        if status & MAC_INTERRUPT_MMC_RX != 0 {
            self.read(MMC_RX_INTERRUPT);
        }
        if status & MAC_INTERRUPT_MMC_TX != 0 {
            self.read(MMC_TX_INTERRUPT);
        }
        if status & MAC_INTERRUPT_MMC_IPC != 0 {
            self.read(MMC_RX_IPC_INTERRUPT);
        }
        (status, self.read(MAC_INTERRUPT_STATUS))
    }

    pub(super) fn quiesce(&self) -> QuiesceSnapshot {
        self.suppress_interrupts();
        let (mac_status_before, _) = self.service_mac_interrupts();
        // Sample and acknowledge causes before the ST/SR clear below. Only
        // causes observed after that stop linearization may use the cleanup
        // classification for TPS/RPS.
        let active_status = self.status();
        let active_legal = active_status & super::protocol::CSR5_W1C_MASK;
        let mut w1c_samples = 0u32;
        if active_legal != 0 {
            self.acknowledge_causes(active_legal);
            w1c_samples = 1;
        }
        // Match Linux's legacy stop_all_dma ordering: RX first, then TX.
        let mut dma = self.read(DMA_CONTROL);
        dma &= !DMA_RX_START;
        self.write(DMA_CONTROL, dma);
        dma &= !DMA_TX_START;
        self.write(DMA_CONTROL, dma);
        let mut mac = self.read(MAC_CONTROL);
        mac &= !(MAC_TX_ENABLE | MAC_RX_ENABLE | MAC_LOOPBACK);
        self.write(MAC_CONTROL, mac);

        let start = MonotonicInstant::now();
        let mut cleanup_legal = 0;
        loop {
            let before = self.status();
            let legal = before & super::protocol::CSR5_W1C_MASK;
            cleanup_legal |= legal;
            if legal != 0 {
                self.acknowledge_causes(legal);
                w1c_samples = w1c_samples.saturating_add(1);
            }
            let status = self.status();
            let tx_process = (status & DMA_TX_PROCESS_MASK) >> DMA_TX_PROCESS_SHIFT;
            let rx_process = (status & DMA_RX_PROCESS_MASK) >> DMA_RX_PROCESS_SHIFT;
            let control = self.read(DMA_CONTROL);
            let mac_control = self.read(MAC_CONTROL);
            let stopped = tx_process == 0
                && rx_process == 0
                && control & (DMA_TX_START | DMA_RX_START) == 0
                && mac_control & (MAC_TX_ENABLE | MAC_RX_ENABLE) == 0;
            if (stopped && status & super::protocol::CSR5_W1C_MASK == 0)
                || start.elapsed() >= Duration::from_millis(DWMAC1000_PROBE_TIMEOUT_MS)
            {
                let (_, mac_status_after) = self.service_mac_interrupts();
                return QuiesceSnapshot {
                    status,
                    active_legal,
                    cleanup_legal,
                    uncleared: quiesce_uncleared(status, stopped),
                    w1c_samples,
                    control,
                    mac_control,
                    tx_process,
                    rx_process,
                    stopped,
                    mac_status_before,
                    mac_status_after,
                };
            }
            core::hint::spin_loop();
        }
    }
}

const fn quiesce_uncleared(status: u32, stopped: bool) -> u32 {
    if stopped {
        status & super::protocol::CSR5_W1C_MASK
    } else {
        0
    }
}

fn mdio_command(clock_range: u32, phy: u8, register: u8, write: bool) -> u32 {
    assert!(clock_range & !MII_CLOCK_RANGE_MASK == 0);
    assert!(phy <= 0x1f && register <= 0x1f);
    MII_BUSY
        | if write { 1 << 1 } else { 0 }
        | (register as u32) << 6
        | (phy as u32) << 11
        | clock_range
}

const fn select_dma_mode(policy: DmaOperationMode, tx_checksum: bool) -> SelectedDmaMode {
    match policy {
        DmaOperationMode::ForceThreshold => SelectedDmaMode::ThresholdBoth,
        DmaOperationMode::ForceStoreForward => SelectedDmaMode::StoreForwardBoth,
        DmaOperationMode::HardwareDefault if tx_checksum => SelectedDmaMode::StoreForwardBoth,
        DmaOperationMode::HardwareDefault => SelectedDmaMode::ThresholdTxStoreForwardRx,
    }
}

const fn dma_mode_bits(mode: SelectedDmaMode) -> u32 {
    match mode {
        SelectedDmaMode::ThresholdBoth => 0,
        SelectedDmaMode::StoreForwardBoth => {
            DMA_TX_STORE_FORWARD | DMA_RX_STORE_FORWARD | DMA_OPERATE_SECOND_FRAME
        },
        SelectedDmaMode::ThresholdTxStoreForwardRx => DMA_RX_STORE_FORWARD,
    }
}

const fn dma_mode_mask() -> u32 {
    DMA_TX_STORE_FORWARD
        | DMA_RX_STORE_FORWARD
        | DMA_OPERATE_SECOND_FRAME
        | DMA_TX_THRESHOLD_MASK
        | DMA_RX_THRESHOLD_MASK
}

const fn fifo_flow_control_expected(rx_fifo_bytes: Option<u32>) -> bool {
    matches!(rx_fifo_bytes, Some(bytes) if bytes >= 4096)
}

const fn configure_fifo_flow_control(control: u32, rx_fifo_bytes: Option<u32>) -> u32 {
    let control = control
        & !(DMA_FLOW_CONTROL_ENABLE | DMA_FLOW_ACTIVATION_MASK | DMA_FLOW_DEACTIVATION_MASK);
    if fifo_flow_control_expected(rx_fifo_bytes) {
        // Linux DWMAC1000 selects full-minus-1K activation and
        // full-minus-2K deactivation for every exact FIFO >= 4 KiB.
        control | DMA_FLOW_CONTROL_ENABLE | 0x0000_0800
    } else {
        control
    }
}

const fn fifo_flow_control_enabled(control: u32) -> bool {
    control & DMA_FLOW_CONTROL_ENABLE != 0
}

const fn effective_mac_interrupt_mask(capabilities: Dwmac1000Capabilities) -> u32 {
    let mut mask = MASK_LEGACY_MAC_INTERRUPTS_REQUESTED;
    // DWMAC1000 drops mask bits for absent optional interrupt sources on
    // readback. Derive the effective value from DMA_HW_FEATURE rather than a
    // board-observed constant.
    if !capabilities.pcs {
        mask &= !(MAC_INTERRUPT_PCS_LINK | MAC_INTERRUPT_PCS_AN);
    }
    if !(capabilities.remote_wake || capabilities.magic_wake) {
        mask &= !MAC_INTERRUPT_PMT;
    }
    if !(capabilities.timestamp_v1 || capabilities.timestamp_v2) {
        mask &= !(1 << 9);
    }
    mask
}

const fn mac_flow_control(link: PhyLink) -> u32 {
    MAC_FLOW_UNICAST_PAUSE
        | if link.full_duplex {
            MAC_FLOW_PAUSE_TIME
        } else {
            0
        }
        | if link.rx_pause { MAC_FLOW_RX_ENABLE } else { 0 }
        | if link.tx_pause { MAC_FLOW_TX_ENABLE } else { 0 }
}

const fn mac_link_bits(speed_mbps: u32, full_duplex: bool) -> Result<u32, SysError> {
    let speed = match speed_mbps {
        1000 => 0,
        100 => MAC_PORT_SELECT | MAC_FAST_ETHERNET_SPEED,
        10 => MAC_PORT_SELECT,
        _ => return Err(SysError::InvalidArgument),
    };
    Ok(speed | if full_duplex { MAC_FULL_DUPLEX } else { 0 })
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ProbeRegisterSnapshot {
    pub(super) bus_mode: u32,
    pub(super) axi_bus_mode: Option<u32>,
    pub(super) rx_desc: u32,
    pub(super) tx_desc: u32,
    pub(super) mac_high: u32,
    pub(super) mac_low: u32,
    pub(super) dma_control: u32,
    pub(super) mac_control: u32,
    pub(super) flow_control: u32,
    pub(super) interrupt_enable: u32,
    pub(super) mac_interrupt_mask: u32,
    pub(super) mmc_rx_interrupt_mask: u32,
    pub(super) mmc_tx_interrupt_mask: u32,
    pub(super) mmc_rx_ipc_interrupt_mask: u32,
    pub(super) mmc_control: u32,
    pub(super) hash_high: u32,
    pub(super) hash_low: u32,
    pub(super) frame_filter: u32,
    pub(super) vlan_tag: u32,
    pub(super) pmt: u32,
    pub(super) lpi_control_status: u32,
    pub(super) timestamp_control: u32,
    /// Diagnostic-only readback. Gate 2 deliberately leaves the Linux RX
    /// watchdog register unprogrammed (`riwt_off` crop).
    pub(super) rx_watchdog: u32,
    pub(super) pcs_selected: bool,
    pub(super) pcs_initial_speed: Option<u32>,
    pub(super) pcs_initial_control: Option<u32>,
    pub(super) pcs_an_control: u32,
    pub(super) pcs_an_status: u32,
    pub(super) selected_dma_mode: SelectedDmaMode,
    pub(super) status: u32,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RuntimeRegisterSnapshot {
    pub(super) status: u32,
    pub(super) dma_control: u32,
    pub(super) interrupt_enable: u32,
    pub(super) mac_control: u32,
    pub(super) mac_interrupt_mask: u32,
    pub(super) mac_address_high: u32,
    pub(super) mac_address_low: u32,
    pub(super) frame_filter: u32,
}

const fn decode_capabilities(version: u32, hw_feature: u32) -> Dwmac1000Capabilities {
    Dwmac1000Capabilities {
        version,
        hw_feature,
        enhanced_descriptors: hw_feature & ENHDESSEL != 0,
        mii: hw_feature & HW_MII != 0,
        gmii: hw_feature & HW_GMII != 0,
        half_duplex: hw_feature & HW_HALF_DUPLEX != 0,
        pcs: hw_feature & HW_PCS != 0,
        mdio: hw_feature & HW_MDIO != 0,
        remote_wake: hw_feature & HW_REMOTE_WAKE != 0,
        magic_wake: hw_feature & HW_MAGIC_WAKE != 0,
        rmon: hw_feature & HW_MMC != 0,
        timestamp_v1: hw_feature & HW_TIMESTAMP_V1 != 0,
        timestamp_v2: hw_feature & HW_TIMESTAMP_V2 != 0,
        eee: hw_feature & HW_EEE != 0,
        tx_checksum: hw_feature & HW_TX_CHECKSUM != 0,
        rx_checksum_type1: hw_feature & HW_RX_CHECKSUM_TYPE1 != 0,
        rx_checksum_type2: hw_feature & HW_RX_CHECKSUM_TYPE2 != 0,
        rx_fifo_over_2048: hw_feature & HW_RX_FIFO_OVER_2048 != 0,
        rx_channels: (((hw_feature & HW_RX_CHANNEL_MASK) >> 20) + 1) as u8,
        tx_channels: (((hw_feature & HW_TX_CHANNEL_MASK) >> 22) + 1) as u8,
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn gate2_admission_requires_linux_enhanced_descriptor_capability() {
        let base = HW_MII | HW_MDIO;
        assert!(!decode_capabilities(0x37, base).supported_gate2());
        assert!(decode_capabilities(0x37, base | ENHDESSEL).supported_gate2());
    }

    #[kunit]
    fn linux_dma_mode_precedence_uses_force_flags_then_tx_checksum() {
        assert_eq!(
            select_dma_mode(DmaOperationMode::ForceThreshold, true),
            SelectedDmaMode::ThresholdBoth
        );
        assert_eq!(
            select_dma_mode(DmaOperationMode::ForceStoreForward, false),
            SelectedDmaMode::StoreForwardBoth
        );
        assert_eq!(
            select_dma_mode(DmaOperationMode::HardwareDefault, true),
            SelectedDmaMode::StoreForwardBoth
        );
        assert_eq!(
            select_dma_mode(DmaOperationMode::HardwareDefault, false),
            SelectedDmaMode::ThresholdTxStoreForwardRx
        );
    }

    #[kunit]
    fn fifo_flow_control_requires_exact_dt_depth_of_at_least_four_kib() {
        assert!(!fifo_flow_control_enabled(configure_fifo_flow_control(
            u32::MAX,
            None
        )));
        assert!(!fifo_flow_control_enabled(configure_fifo_flow_control(
            u32::MAX,
            Some(2048)
        )));
        let enabled = configure_fifo_flow_control(0, Some(4096));
        assert!(fifo_flow_control_enabled(enabled));
        assert_eq!(enabled & DMA_FLOW_DEACTIVATION_MASK, 0x800);
    }

    #[kunit]
    fn capability_decode_preserves_linux_channel_and_optional_feature_facts() {
        let capabilities = decode_capabilities(
            EXPECTED_CORE_VERSION as u32,
            HW_MII
                | HW_GMII
                | HW_MDIO
                | HW_MMC
                | HW_TX_CHECKSUM
                | ENHDESSEL
                | (2 << 20)
                | (1 << 22),
        );
        assert!(!capabilities.supported_gate2());
        assert!(capabilities.rmon);
        assert!(capabilities.tx_checksum);
        assert_eq!(capabilities.rx_channels, 3);
        assert_eq!(capabilities.tx_channels, 2);
    }

    #[kunit]
    fn mac_interrupt_mask_readback_tracks_optional_capabilities() {
        let base = HW_MII | HW_MDIO | HW_REMOTE_WAKE | HW_TIMESTAMP_V2;
        assert_eq!(
            effective_mac_interrupt_mask(decode_capabilities(EXPECTED_CORE_VERSION as u32, base)),
            0x209
        );
        assert_eq!(
            effective_mac_interrupt_mask(decode_capabilities(
                EXPECTED_CORE_VERSION as u32,
                base | HW_PCS
            )),
            MASK_LEGACY_MAC_INTERRUPTS_REQUESTED
        );
        assert_eq!(
            effective_mac_interrupt_mask(decode_capabilities(
                EXPECTED_CORE_VERSION as u32,
                HW_MII | HW_MDIO
            )),
            MAC_INTERRUPT_RGMII
        );
    }

    #[kunit]
    fn mdio_read_command_keeps_divider_phy_and_register_fields_separate() {
        let command = mdio_command(2 << MII_CLOCK_RANGE_SHIFT, 3, 2, false);
        assert_eq!(command & 1, MII_BUSY);
        assert_eq!(command & MII_CLOCK_RANGE_MASK, 2 << MII_CLOCK_RANGE_SHIFT);
        assert_eq!((command >> 6) & 0x1f, 2);
        assert_eq!((command >> 11) & 0x1f, 3);
    }

    #[kunit]
    fn quiesce_timeout_without_stop_does_not_claim_w1c_failure() {
        assert_eq!(
            quiesce_uncleared(super::super::protocol::CSR5_W1C_MASK, false),
            0
        );
    }

    #[kunit]
    fn stopped_quiesce_reports_final_legal_residue() {
        assert_eq!(quiesce_uncleared((1 << 6) | (1 << 20), true), 1 << 6);
    }

    #[kunit]
    fn capability_admission_requires_the_370_family_version() {
        assert!(decode_capabilities(EXPECTED_CORE_VERSION as u32, 0).expected_family());
        assert!(!decode_capabilities(0x36, 0).expected_family());
    }

    #[kunit]
    fn atds_readback_is_derived_from_dma_bus_mode() {
        assert!(!Dwmac1000Regs::atds(0));
        assert!(Dwmac1000Regs::atds(ALTERNATE_DESCRIPTOR_SIZE));
    }

    #[kunit]
    fn legacy_mac_link_bits_cover_only_clause_22_speeds() {
        assert_eq!(mac_link_bits(1000, true), Ok(MAC_FULL_DUPLEX));
        assert_eq!(
            mac_link_bits(100, false),
            Ok(MAC_PORT_SELECT | MAC_FAST_ETHERNET_SPEED)
        );
        assert_eq!(mac_link_bits(10, false), Ok(MAC_PORT_SELECT));
        assert_eq!(mac_link_bits(2500, true), Err(SysError::InvalidArgument));
    }
}
