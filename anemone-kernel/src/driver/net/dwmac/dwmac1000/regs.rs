use core::sync::atomic::AtomicU32;

use crate::{mm::remap::IoRemap, prelude::*, time::MonotonicInstant};

use super::{
    fwnode::{DmaOperationMode, LegacyAxiConfig, LegacyDmaConfig, MdioClockRange},
    phy::{Clause22Register, PhyLink},
    protocol::DmaStatus,
};

#[derive(Clone, Copy, Debug)]
#[repr(usize)]
enum Register {
    MacControl = 0x0000,
    MacFrameFilter = 0x0004,
    MacHashHigh = 0x0008,
    MacHashLow = 0x000c,
    MacMiiAddress = 0x0010,
    MacMiiData = 0x0014,
    MacFlowControl = 0x0018,
    MacVlanTag = 0x001c,
    MacVersion = 0x0020,
    MacPmt = 0x002c,
    MacLpiControlStatus = 0x0030,
    MacInterruptStatus = 0x0038,
    MacInterruptMask = 0x003c,
    MacAddressHigh = 0x0040,
    MacAddressLow = 0x0044,
    MacAnControl = 0x00c0,
    MacAnStatus = 0x00c4,
    MacRgmiiStatus = 0x00d8,
    MmcControl = 0x0100,
    MmcRxInterrupt = 0x0104,
    MmcTxInterrupt = 0x0108,
    MmcRxInterruptMask = 0x010c,
    MmcTxInterruptMask = 0x0110,
    // Linux expresses these counters relative to MAC + 0x100. Reset-on-read
    // makes each value an interval snapshot between diagnostic reads.
    MmcRxFrameCountGb = 0x0180,
    MmcRxCrcError = 0x0194,
    MmcRxAlignError = 0x0198,
    MmcRxRunError = 0x019c,
    MmcRxUnicast = 0x01c4,
    MmcRxLengthError = 0x01c8,
    MmcRxFifoOverflow = 0x01d4,
    MmcRxWatchdogError = 0x01dc,
    MmcRxIpcInterruptMask = 0x0200,
    MmcRxIpcInterrupt = 0x0208,
    PtpTimestampControl = 0x0700,
    DmaBusMode = 0x1000,
    DmaTxPollDemand = 0x1004,
    DmaRxPollDemand = 0x1008,
    DmaRxBaseAddress = 0x100c,
    DmaTxBaseAddress = 0x1010,
    DmaStatus = 0x1014,
    DmaControl = 0x1018,
    DmaInterruptEnable = 0x101c,
    // Linux legacy DWMAC1000 diagnostics; these registers are read-only here.
    DmaMissedFrameCounter = 0x1020,
    DmaRxWatchdog = 0x1024,
    DmaAxiBusMode = 0x1028,
    DmaCurrentTxBuffer = 0x1050,
    DmaCurrentRxBuffer = 0x1054,
    DmaHardwareFeature = 0x1058,
}

const EXPECTED_CORE_VERSION: u8 = 0x37;
const MII_CLOCK_RANGE_SHIFT: u32 = 2;
const DMA_TX_PROCESS_SHIFT: u32 = 20;
const DMA_RX_PROCESS_SHIFT: u32 = 17;
const DMA_PBL_SHIFT: u32 = 8;
const DMA_RX_PBL_SHIFT: u32 = 17;
const COSMOS_AXI_BUS_MODE: u32 = 0x0077_00ff;
const PHY_ID_YT8511: u32 = 0x0000_010a;

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct DmaBusMode: u32 {
        const SOFTWARE_RESET = 1;
        const ALTERNATE_DESCRIPTOR_SIZE = 1 << 7;
        const TX_PBL_MASK = 0x3f << 8;
        const BURST_LENGTH32 = 1 << 13;
        const FIXED_BURST = 1 << 16;
        const RX_PBL_MASK = 0x3f << 17;
        const USE_SEPARATE_PBL = 1 << 23;
        const PBL_X8 = 1 << 24;
        const ADDRESS_ALIGNED_BEATS = 1 << 25;
        const MIXED_BURST = 1 << 26;
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct MdioAddress: u32 {
        const BUSY = 1;
        const CLOCK_RANGE_MASK = 0xf << MII_CLOCK_RANGE_SHIFT;
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct HardwareFeature: u32 {
        const MII = 1 << 0;
        const GMII = 1 << 1;
        const HALF_DUPLEX = 1 << 2;
        const PCS = 1 << 6;
        const MDIO = 1 << 8;
        const REMOTE_WAKE = 1 << 9;
        const MAGIC_WAKE = 1 << 10;
        const MMC = 1 << 11;
        const TIMESTAMP_V1 = 1 << 12;
        const TIMESTAMP_V2 = 1 << 13;
        const EEE = 1 << 14;
        const TX_CHECKSUM = 1 << 16;
        const RX_CHECKSUM_TYPE1 = 1 << 17;
        const RX_CHECKSUM_TYPE2 = 1 << 18;
        const RX_FIFO_OVER_2048 = 1 << 19;
        const RX_CHANNEL_MASK = 0x3 << 20;
        const TX_CHANNEL_MASK = 0x3 << 22;
        const ENHANCED_DESCRIPTORS = 1 << 24;
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct MacControl: u32 {
        const RX_ENABLE = 1 << 2;
        const TX_ENABLE = 1 << 3;
        const AUTO_PAD_FCS_STRIP = 1 << 7;
        const RX_CHECKSUM = 1 << 10;
        const FULL_DUPLEX = 1 << 11;
        const LOOPBACK = 1 << 12;
        const FAST_ETHERNET_SPEED = 1 << 14;
        const PORT_SELECT = 1 << 15;
        const DISABLE_CARRIER_SENSE = 1 << 16;
        const JUMBO_ENABLE = 1 << 20;
        const FRAME_BURST = 1 << 21;
        const JABBER_DISABLE = 1 << 22;
        const WATCHDOG_DISABLE = 1 << 23;
        const FRAME_2K_ENABLE = 1 << 27;
        const LINK_MASK = Self::PORT_SELECT.bits()
            | Self::FAST_ETHERNET_SPEED.bits()
            | Self::FULL_DUPLEX.bits();
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct DmaControl: u32 {
        const RX_START = 1 << 1;
        const OPERATE_SECOND_FRAME = 1 << 2;
        const RX_THRESHOLD_MASK = 0x3 << 3;
        const FLOW_CONTROL_ENABLE = 1 << 8;
        const TX_START = 1 << 13;
        const TX_THRESHOLD_MASK = 0x7 << 14;
        const FLOW_DEACTIVATION_MASK = 0x0040_1800;
        const FLOW_ACTIVATION_MASK = 0x0080_0600;
        const TX_STORE_FORWARD = 1 << 21;
        const RX_STORE_FORWARD = 1 << 25;
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct MacFrameFilter: u32 {
        // Linux's legacy DWMAC1000 default with the primary address populated.
        const HASH_OR_PERFECT = 1 << 10;
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct MacFlowControl: u32 {
        const TX_ENABLE = 1 << 1;
        const RX_ENABLE = 1 << 2;
        const UNICAST_PAUSE = 1 << 3;
        const PAUSE_TIME_MASK = 0xffff << 16;
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct MacPowerManagement: u32 {
        const ENABLE_MASK = (1 << 9) | (1 << 2) | (1 << 1) | 1;
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct MacLpiControl: u32 {
        const ENABLE_MASK = (1 << 19) | (1 << 16);
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct MacAutoNegotiation: u32 {
        const ENABLE_RESTART = (1 << 12) | (1 << 9);
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct MacInterrupt: u32 {
        const RGMII = 1 << 0;
        const PCS_LINK = 1 << 1;
        const PCS_AUTO_NEGOTIATION = 1 << 2;
        const POWER_MANAGEMENT = 1 << 3;
        const MMC_RX = 1 << 5;
        const MMC_TX = 1 << 6;
        const MMC_IPC = 1 << 7;
        const LPI = 1 << 10;
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct MacInterruptMask: u32 {
        // The probe consumes RGMII status while masking other legacy sources.
        const LEGACY_REQUESTED = 0x20f;
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct MmcControl: u32 {
        const LINUX_INITIAL = 0x35;
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct MmcInterruptMask: u32 {
        const _ = !0;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct DmaInterruptEnable: u32 {
        const TX = 1;
        const RX = 1 << 6;
        const ABNORMAL = 1 << 15;
        const NORMAL = 1 << 16;
        const _ = !0;
    }
}

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
        self.mac_enabled_control & (MacControl::TX_ENABLE.bits() | MacControl::RX_ENABLE.bits())
            == MacControl::TX_ENABLE.bits() | MacControl::RX_ENABLE.bits()
            && self.mac_enabled_control & MacControl::LOOPBACK.bits() == 0
            && self.rx_started_control & (DmaControl::RX_START.bits() | DmaControl::TX_START.bits())
                == DmaControl::RX_START.bits()
            && self.tx_started_control & (DmaControl::RX_START.bits() | DmaControl::TX_START.bits())
                == DmaControl::RX_START.bits() | DmaControl::TX_START.bits()
            && self.hash_high == 0
            && self.hash_low == 0
            && self.frame_filter == MacFrameFilter::HASH_OR_PERFECT.bits()
            && self.loopback_control
                & (MacControl::TX_ENABLE.bits()
                    | MacControl::RX_ENABLE.bits()
                    | MacControl::LOOPBACK.bits())
                == MacControl::TX_ENABLE.bits()
                    | MacControl::RX_ENABLE.bits()
                    | MacControl::LOOPBACK.bits()
            && self.interrupt_enable == 0
    }
}

pub(super) struct Dwmac1000Regs {
    remap: IoRemap,
    mdio_clock_range: AtomicU32,
}

impl Dwmac1000Regs {
    const REQUIRED_MAPPING_LEN: usize = Register::DmaHardwareFeature as usize + 4;

    pub(super) fn new(remap: IoRemap) -> Result<Self, SysError> {
        if remap.size() < Self::REQUIRED_MAPPING_LEN as u64 {
            return Err(SysError::DriverIncompatible);
        }
        Ok(Self {
            remap,
            mdio_clock_range: AtomicU32::new(0),
        })
    }

    fn ptr_at(&self, register: Register) -> *mut u32 {
        let offset = register as usize;
        let end = offset.checked_add(core::mem::size_of::<u32>()).unwrap();
        assert!(end <= self.remap.size() as usize);
        unsafe { self.remap.as_ptr().as_ptr().cast::<u8>().add(offset).cast() }
    }

    fn read(&self, register: Register) -> u32 {
        core::sync::atomic::fence(Ordering::SeqCst);
        let value = unsafe { core::ptr::read_volatile(self.ptr_at(register)) };
        core::sync::atomic::fence(Ordering::SeqCst);
        value
    }

    fn write(&self, register: Register, value: u32) {
        core::sync::atomic::fence(Ordering::SeqCst);
        unsafe { core::ptr::write_volatile(self.ptr_at(register), value) }
    }

    pub(super) fn capabilities(&self) -> Dwmac1000Capabilities {
        let version = self.read(Register::MacVersion);
        let hw_feature = self.read(Register::DmaHardwareFeature);
        decode_capabilities(version, hw_feature)
    }

    pub(super) fn reset_dma(&self) -> Result<DmaResetSnapshot, DmaResetSnapshot> {
        let before = self.read(Register::DmaBusMode);
        let mut mode = before;
        mode |= DmaBusMode::SOFTWARE_RESET.bits();
        self.write(Register::DmaBusMode, mode);
        let start = MonotonicInstant::now();
        loop {
            let current = self.read(Register::DmaBusMode);
            if current & DmaBusMode::SOFTWARE_RESET.bits() == 0 {
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
        bus_mode & DmaBusMode::ALTERNATE_DESCRIPTOR_SIZE.bits() != 0
    }

    pub(super) fn mdio_clock_range(&self) -> Option<MdioClockRange> {
        MdioClockRange::new(self.mdio_clock_range_raw() as u32)
    }

    pub(super) fn mdio_clock_range_raw(&self) -> u8 {
        ((self.read(Register::MacMiiAddress) & MdioAddress::CLOCK_RANGE_MASK.bits())
            >> MII_CLOCK_RANGE_SHIFT) as u8
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
            id1: self.mdio_read(phy, Clause22Register::PhyId1)?,
            id2: self.mdio_read(phy, Clause22Register::PhyId2)?,
            bmcr: self.mdio_read(phy, Clause22Register::BasicControl)?,
            bmsr: self.mdio_read(phy, Clause22Register::BasicStatus)?,
        })
    }

    pub(super) fn mdio_read(&self, phy: u8, register: Clause22Register) -> Result<u16, SysError> {
        self.wait_mdio_idle()?;
        self.write(
            Register::MacMiiAddress,
            self.mdio_command(phy, register as u8, false),
        );
        self.wait_mdio_idle()?;
        Ok(self.read(Register::MacMiiData) as u16)
    }

    pub(super) fn mdio_write(
        &self,
        phy: u8,
        register: Clause22Register,
        value: u16,
    ) -> Result<(), SysError> {
        self.wait_mdio_idle()?;
        self.write(Register::MacMiiData, value as u32);
        self.write(
            Register::MacMiiAddress,
            self.mdio_command(phy, register as u8, true),
        );
        self.wait_mdio_idle()
    }

    fn wait_mdio_idle(&self) -> Result<(), SysError> {
        let start = MonotonicInstant::now();
        loop {
            if self.read(Register::MacMiiAddress) & MdioAddress::BUSY.bits() == 0 {
                return Ok(());
            }
            if start.elapsed() >= Duration::from_millis(DWMAC1000_MDIO_TIMEOUT_MS) {
                return Err(SysError::Timeout);
            }
            core::hint::spin_loop();
        }
    }

    pub(super) fn mdio_address(&self) -> u32 {
        self.read(Register::MacMiiAddress)
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
        self.write(Register::DmaInterruptEnable, 0);
        self.write(
            Register::MacInterruptMask,
            MacInterruptMask::LEGACY_REQUESTED.bits(),
        );
        self.mask_mmc_interrupts();
        let mut bus_mode = self.read(Register::DmaBusMode);
        bus_mode &= !(DmaBusMode::ALTERNATE_DESCRIPTOR_SIZE.bits()
            | DmaBusMode::TX_PBL_MASK.bits()
            | DmaBusMode::RX_PBL_MASK.bits()
            | DmaBusMode::USE_SEPARATE_PBL.bits()
            | DmaBusMode::PBL_X8.bits()
            | DmaBusMode::FIXED_BURST.bits()
            | DmaBusMode::MIXED_BURST.bits()
            | DmaBusMode::ADDRESS_ALIGNED_BEATS.bits());
        bus_mode |= DmaBusMode::ALTERNATE_DESCRIPTOR_SIZE.bits()
            | DmaBusMode::USE_SEPARATE_PBL.bits()
            | (dma.tx_pbl.encoded() as u32) << DMA_PBL_SHIFT
            | (dma.rx_pbl.encoded() as u32) << DMA_RX_PBL_SHIFT
            // Cosmos uses mixed bursts, BLEN32 and PBLx8 on this controller.
            // Keep ATDS/PBL from the accepted enhanced path, but force these
            // three bus attributes for this board comparison experiment.
            | DmaBusMode::MIXED_BURST.bits()
            | DmaBusMode::BURST_LENGTH32.bits()
            | DmaBusMode::PBL_X8.bits();
        // The Cosmos comparison above intentionally overrides the firmware
        // default for PBLx8; retain the parsed fields for diagnostics only.
        let _ = dma.pbl_x8;
        if dma.fixed_burst {
            bus_mode |= DmaBusMode::FIXED_BURST.bits();
        }
        if dma.mixed_burst {
            bus_mode |= DmaBusMode::MIXED_BURST.bits();
        }
        if dma.address_aligned_beats {
            bus_mode |= DmaBusMode::ADDRESS_ALIGNED_BEATS.bits();
        }
        self.write(Register::DmaBusMode, bus_mode);
        // Temporary comparison baseline: Cosmos programs this AXI value even
        // when the 2K1000 DT has no snps,axi-config phandle. Do not treat this
        // as the Linux-shaped policy until a successful hardware rerun proves
        // that it is required.
        let _ = axi;
        self.write(Register::DmaAxiBusMode, COSMOS_AXI_BUS_MODE);
        let axi_bus_mode = Some(self.read(Register::DmaAxiBusMode));

        let selected_dma_mode = select_dma_mode(dma.operation_mode, capabilities.tx_checksum);
        let mut dma_control = self.read(Register::DmaControl);
        dma_control &= !(DmaControl::TX_START.bits()
            | DmaControl::RX_START.bits()
            | DmaControl::TX_STORE_FORWARD.bits()
            | DmaControl::RX_STORE_FORWARD.bits()
            | DmaControl::OPERATE_SECOND_FRAME.bits()
            | DmaControl::TX_THRESHOLD_MASK.bits()
            | DmaControl::RX_THRESHOLD_MASK.bits()
            | DmaControl::FLOW_CONTROL_ENABLE.bits()
            | DmaControl::FLOW_ACTIVATION_MASK.bits()
            | DmaControl::FLOW_DEACTIVATION_MASK.bits());
        dma_control |= dma_mode_bits(selected_dma_mode);
        dma_control = configure_fifo_flow_control(dma_control, dma.rx_fifo_bytes);
        self.write(Register::DmaControl, dma_control);

        let mut mac_control = self.read(Register::MacControl);
        mac_control &= !(MacControl::TX_ENABLE.bits()
            | MacControl::RX_ENABLE.bits()
            | MacControl::LOOPBACK.bits()
            | MacControl::LINK_MASK.bits()
            | MacControl::AUTO_PAD_FCS_STRIP.bits()
            | MacControl::RX_CHECKSUM.bits()
            | MacControl::JUMBO_ENABLE.bits()
            | MacControl::FRAME_2K_ENABLE.bits()
            | MacControl::WATCHDOG_DISABLE.bits());
        let pcs_initial_control = if capabilities.pcs {
            if let Some(speed) = pcs_initial_speed {
                mac_control |= mac_link_bits(speed, false)?;
                self.write(Register::MacControl, mac_control);
                Some(self.read(Register::MacControl))
            } else {
                None
            }
        } else {
            None
        };
        mac_control &= !MacControl::LINK_MASK.bits();
        // Linux programs PS/FES/DM from the resolved PHY link before enabling
        // the MAC. Internal loopback still consumes that MAC clock selection.
        mac_control |= MacControl::JABBER_DISABLE.bits()
            | MacControl::FRAME_BURST.bits()
            | MacControl::DISABLE_CARRIER_SENSE.bits()
            | mac_link_bits(link.speed_mbps, link.full_duplex)?;
        self.write(Register::MacControl, mac_control);
        self.write(Register::MacFlowControl, mac_flow_control(link));
        self.write(Register::DmaRxBaseAddress, rx_desc);
        self.write(Register::DmaTxBaseAddress, tx_desc);
        self.write(
            Register::MacAddressHigh,
            ((mac[5] as u32) << 8) | mac[4] as u32 | (1 << 31),
        );
        self.write(
            Register::MacAddressLow,
            ((mac[3] as u32) << 24)
                | ((mac[2] as u32) << 16)
                | ((mac[1] as u32) << 8)
                | mac[0] as u32,
        );
        // Empty address lists use address 0 perfect matching. Program this
        // before any RX path can run so loopback traffic cannot observe the
        // firmware filter state.
        self.write(Register::MacHashHigh, 0);
        self.write(Register::MacHashLow, 0);
        self.write(
            Register::MacFrameFilter,
            MacFrameFilter::HASH_OR_PERFECT.bits(),
        );

        self.write(Register::MacVlanTag, 0);
        let has_pmt = capabilities.remote_wake || capabilities.magic_wake;
        let pmt = if has_pmt {
            self.write(Register::MacPmt, 0);
            self.read(Register::MacPmt)
        } else {
            0
        };
        let lpi_control_status = if capabilities.eee {
            let value =
                self.read(Register::MacLpiControlStatus) & !MacLpiControl::ENABLE_MASK.bits();
            self.write(Register::MacLpiControlStatus, value);
            self.read(Register::MacLpiControlStatus)
        } else {
            0
        };
        let has_timestamp = capabilities.timestamp_v1 || capabilities.timestamp_v2;
        let timestamp_control = if has_timestamp {
            self.write(Register::PtpTimestampControl, 0);
            self.read(Register::PtpTimestampControl)
        } else {
            0
        };
        self.initialize_mmc(capabilities.rmon);

        let pcs_selected = capabilities.pcs;
        let (pcs_an_control, pcs_an_status) = if pcs_selected {
            let mut an_control = self.read(Register::MacAnControl);
            an_control |= MacAutoNegotiation::ENABLE_RESTART.bits();
            self.write(Register::MacAnControl, an_control);
            (
                self.read(Register::MacAnControl),
                self.read(Register::MacAnStatus),
            )
        } else {
            (0, 0)
        };
        self.acknowledge_causes(self.status() & DmaStatus::W1C.bits());

        // Linux writes all mask bits. Legacy DWMAC1000 exposes only the
        // implemented counter fields on readback, so an effective mask is
        // diagnostic evidence rather than an exact u32::MAX admission value.
        let snapshot = ProbeRegisterSnapshot {
            bus_mode: self.read(Register::DmaBusMode),
            axi_bus_mode,
            rx_desc: self.read(Register::DmaRxBaseAddress),
            tx_desc: self.read(Register::DmaTxBaseAddress),
            mac_high: self.read(Register::MacAddressHigh),
            mac_low: self.read(Register::MacAddressLow),
            dma_control: self.read(Register::DmaControl),
            mac_control: self.read(Register::MacControl),
            flow_control: self.read(Register::MacFlowControl),
            interrupt_enable: self.interrupt_enable(),
            mac_interrupt_mask: self.read(Register::MacInterruptMask),
            mmc_rx_interrupt_mask: self.read(Register::MmcRxInterruptMask),
            mmc_tx_interrupt_mask: self.read(Register::MmcTxInterruptMask),
            mmc_rx_ipc_interrupt_mask: self.read(Register::MmcRxIpcInterruptMask),
            mmc_control: if capabilities.rmon {
                self.read(Register::MmcControl)
            } else {
                0
            },
            hash_high: self.read(Register::MacHashHigh),
            hash_low: self.read(Register::MacHashLow),
            frame_filter: self.read(Register::MacFrameFilter),
            vlan_tag: self.read(Register::MacVlanTag),
            pmt,
            lpi_control_status,
            timestamp_control,
            rx_watchdog: self.read(Register::DmaRxWatchdog),
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
        if snapshot.dma_control & (DmaControl::TX_START.bits() | DmaControl::RX_START.bits()) != 0 {
            mismatch |= 1 << 5;
        }
        if snapshot.interrupt_enable != 0 {
            mismatch |= 1 << 6;
        }
        if snapshot.mac_control
            & (MacControl::TX_ENABLE.bits()
                | MacControl::RX_ENABLE.bits()
                | MacControl::LOOPBACK.bits()
                | MacControl::AUTO_PAD_FCS_STRIP.bits()
                | MacControl::RX_CHECKSUM.bits())
            != 0
        {
            mismatch |= 1 << 7;
        }
        if snapshot.mac_control & MacControl::LINK_MASK.bits()
            != mac_link_bits(link.speed_mbps, link.full_duplex)?
        {
            mismatch |= 1 << 8;
        }
        if snapshot.hash_high != 0
            || snapshot.hash_low != 0
            || snapshot.frame_filter != MacFrameFilter::HASH_OR_PERFECT.bits()
        {
            mismatch |= 1 << 9;
        }
        if snapshot.vlan_tag != 0
            || (has_pmt && snapshot.pmt & MacPowerManagement::ENABLE_MASK.bits() != 0)
            || (capabilities.eee
                && snapshot.lpi_control_status & MacLpiControl::ENABLE_MASK.bits() != 0)
            || (has_timestamp && snapshot.timestamp_control != 0)
        {
            mismatch |= 1 << 10;
        }
        if snapshot.pcs_selected
            && snapshot.pcs_an_control & MacAutoNegotiation::ENABLE_RESTART.bits()
                != MacAutoNegotiation::ENABLE_RESTART.bits()
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
        self.acknowledge_causes(self.status() & DmaStatus::W1C.bits());
        // Gate 2 runs before the architecture enables local interrupts. Keep
        // every device interrupt source suppressed and observe CSR5 directly;
        // Gate 3 owns the first IRQ request and CSR7 enable transition.
        self.suppress_interrupts();
        let hash_high = self.read(Register::MacHashHigh);
        let hash_low = self.read(Register::MacHashLow);
        let frame_filter = self.read(Register::MacFrameFilter);

        let mut mac = self.read(Register::MacControl);
        mac &= !MacControl::LOOPBACK.bits();
        mac |= MacControl::TX_ENABLE.bits() | MacControl::RX_ENABLE.bits();
        self.write(Register::MacControl, mac);
        let mac_enabled_control = self.read(Register::MacControl);

        let mut dma = self.read(Register::DmaControl);
        // Linux starts the legacy RX channel before the TX channel. Keep the
        // two commits distinct so TX cannot consume a descriptor before the
        // receive ring is live.
        dma |= DmaControl::RX_START.bits();
        self.write(Register::DmaControl, dma);
        let rx_started_control = self.read(Register::DmaControl);
        dma |= DmaControl::TX_START.bits();
        self.write(Register::DmaControl, dma);
        let tx_started_control = self.read(Register::DmaControl);
        // Cosmos explicitly kicks the RX state machine after enabling both
        // channels; keep this write in the same start transaction.
        self.demand_rx();

        mac = self.read(Register::MacControl);
        mac |= MacControl::LOOPBACK.bits();
        self.write(Register::MacControl, mac);
        ProbeStartSnapshot {
            mac_enabled_control,
            rx_started_control,
            tx_started_control,
            hash_high,
            hash_low,
            frame_filter,
            loopback_control: self.read(Register::MacControl),
            flow_control: self.read(Register::MacFlowControl),
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
        self.write(Register::DmaTxPollDemand, 1);
    }

    pub(super) fn demand_rx(&self) {
        self.write(Register::DmaRxPollDemand, 1);
    }

    /// Reload the production descriptor origins after Gate 2 advanced the
    /// hardware cursors. Software may reset its ring indices to zero only
    /// after this stopped-DMA transaction succeeds; otherwise hardware would
    /// resume from the post-loopback slot while software waits on slot zero.
    pub(super) fn rearm_descriptor_bases(
        &self,
        rx_desc: u32,
        tx_desc: u32,
    ) -> Result<(), SysError> {
        let control_before = self.read(Register::DmaControl);
        if control_before & (DmaControl::RX_START.bits() | DmaControl::TX_START.bits()) != 0 {
            kerrln!(
                "dwmac1000 stage=gate2-production-rearm result=fail reason=dma-running csr6={:#x} expected-rx={:#x} expected-tx={:#x}",
                control_before,
                rx_desc,
                tx_desc,
            );
            return Err(SysError::ProbeFailed);
        }

        self.write(Register::DmaRxBaseAddress, rx_desc);
        self.write(Register::DmaTxBaseAddress, tx_desc);
        let control_after = self.read(Register::DmaControl);
        let actual_rx = self.read(Register::DmaRxBaseAddress);
        let actual_tx = self.read(Register::DmaTxBaseAddress);
        if !descriptor_rearm_valid(control_after, actual_rx, actual_tx, rx_desc, tx_desc) {
            kerrln!(
                "dwmac1000 stage=gate2-production-rearm result=fail reason=readback csr6={:#x} expected-rx={:#x} actual-rx={:#x} expected-tx={:#x} actual-tx={:#x}",
                control_after,
                rx_desc,
                actual_rx,
                tx_desc,
                actual_tx,
            );
            return Err(SysError::ProbeFailed);
        }
        kdebugln!(
            "dwmac1000 stage=gate2-production-rearm result=pass csr6={:#x} rx-base={:#x} tx-base={:#x} cur-before-start={:#x},{:#x}",
            control_after,
            actual_rx,
            actual_tx,
            self.read(Register::DmaCurrentTxBuffer),
            self.read(Register::DmaCurrentRxBuffer),
        );
        Ok(())
    }

    pub(super) fn start_runtime(&self) {
        // Gate 3 adopts the quiesced Gate 2 owner. Clear stale device causes
        // before enabling CSR7 so the first level dispatch represents a new
        // event, then follow Linux's RX-before-TX start order.
        self.service_mac_interrupts();
        self.acknowledge_causes(self.status() & DmaStatus::W1C.bits());
        let mut mac = self.read(Register::MacControl);
        mac |= MacControl::TX_ENABLE.bits() | MacControl::RX_ENABLE.bits();
        self.write(Register::MacControl, mac);
        let mut dma = self.read(Register::DmaControl);
        dma |= DmaControl::RX_START.bits();
        self.write(Register::DmaControl, dma);
        dma |= DmaControl::TX_START.bits();
        self.write(Register::DmaControl, dma);
        self.demand_rx();
        // The Gate 2 mask is a suppression baseline. Gate 3 restores the
        // Linux host path; read-to-clear MAC/PCS/MMC causes are then drained
        // by the same IRQ handler before level unmask.
        self.write(Register::MacInterruptMask, 0);
        self.write(
            Register::DmaInterruptEnable,
            DmaInterruptEnable::NORMAL.bits()
                | DmaInterruptEnable::ABNORMAL.bits()
                | DmaInterruptEnable::RX.bits()
                | DmaInterruptEnable::TX.bits(),
        );
    }

    pub(super) fn status(&self) -> u32 {
        self.read(Register::DmaStatus)
    }

    pub(super) fn interrupt_enable(&self) -> u32 {
        self.read(Register::DmaInterruptEnable)
    }

    /// Read-only field diagnostics. These values mirror the live MMIO owner
    /// and never participate in interrupt or DMA decisions.
    pub(super) fn runtime_snapshot(&self) -> RuntimeRegisterSnapshot {
        RuntimeRegisterSnapshot {
            status: self.status(),
            dma_control: self.read(Register::DmaControl),
            interrupt_enable: self.interrupt_enable(),
            dma_current_tx_buffer: self.read(Register::DmaCurrentTxBuffer),
            dma_current_rx_buffer: self.read(Register::DmaCurrentRxBuffer),
            dma_missed_frame_counter: self.read(Register::DmaMissedFrameCounter),
            mac_control: self.read(Register::MacControl),
            mac_interrupt_mask: self.read(Register::MacInterruptMask),
            rgmii_status: self.read(Register::MacRgmiiStatus),
            mac_address_high: self.read(Register::MacAddressHigh),
            mac_address_low: self.read(Register::MacAddressLow),
            frame_filter: self.read(Register::MacFrameFilter),
            mmc_rx_frame_count_gb: self.read(Register::MmcRxFrameCountGb),
            mmc_rx_unicast: self.read(Register::MmcRxUnicast),
            mmc_rx_crc_error: self.read(Register::MmcRxCrcError),
            mmc_rx_align_error: self.read(Register::MmcRxAlignError),
            mmc_rx_run_error: self.read(Register::MmcRxRunError),
            mmc_rx_length_error: self.read(Register::MmcRxLengthError),
            mmc_rx_fifo_overflow: self.read(Register::MmcRxFifoOverflow),
            mmc_rx_watchdog_error: self.read(Register::MmcRxWatchdogError),
        }
    }

    pub(super) fn acknowledge_causes(&self, causes: u32) -> u32 {
        if causes != 0 {
            assert_eq!(causes & !DmaStatus::W1C.bits(), 0);
            self.write(Register::DmaStatus, causes);
        }
        // W1C evidence is only valid when the same owner performs the
        // immediate readback. Callers may use this as their next raw sample.
        self.status()
    }

    fn mask_mmc_interrupts(&self) {
        self.write(Register::MmcRxInterruptMask, MmcInterruptMask::all().bits());
        self.write(Register::MmcTxInterruptMask, MmcInterruptMask::all().bits());
        self.write(
            Register::MmcRxIpcInterruptMask,
            MmcInterruptMask::all().bits(),
        );
    }

    fn initialize_mmc(&self, rmon: bool) {
        self.mask_mmc_interrupts();
        if rmon {
            // Linux treats these control bits as an initialization command.
            // COUNTER_RESET/PRESET/FULL_HALF_PRESET may self-clear; only the
            // write is protocol truth, while the post-write value is retained
            // as diagnostic evidence below.
            let control = self.read(Register::MmcControl) | MmcControl::LINUX_INITIAL.bits();
            self.write(Register::MmcControl, control);
        }
    }

    pub(super) fn suppress_interrupts(&self) {
        self.write(Register::DmaInterruptEnable, 0);
        self.write(
            Register::MacInterruptMask,
            MacInterruptMask::LEGACY_REQUESTED.bits(),
        );
        self.mask_mmc_interrupts();
    }

    /// Drain every legacy host cause that Linux services through a
    /// read-to-clear register. Gate 2 keeps these sources masked, but must not
    /// hand stale device state to Gate 3's first unmask.
    pub(super) fn service_mac_interrupts(&self) -> (u32, u32) {
        let status = self.read(Register::MacInterruptStatus);
        if status & (MacInterrupt::PCS_LINK.bits() | MacInterrupt::PCS_AUTO_NEGOTIATION.bits()) != 0
        {
            self.read(Register::MacAnStatus);
        }
        if status & MacInterrupt::RGMII.bits() != 0 {
            self.read(Register::MacRgmiiStatus);
        }
        if status & MacInterrupt::POWER_MANAGEMENT.bits() != 0 {
            self.read(Register::MacPmt);
        }
        if status & MacInterrupt::LPI.bits() != 0 {
            self.read(Register::MacLpiControlStatus);
        }
        if status & MacInterrupt::MMC_RX.bits() != 0 {
            self.read(Register::MmcRxInterrupt);
        }
        if status & MacInterrupt::MMC_TX.bits() != 0 {
            self.read(Register::MmcTxInterrupt);
        }
        if status & MacInterrupt::MMC_IPC.bits() != 0 {
            self.read(Register::MmcRxIpcInterrupt);
        }
        (status, self.read(Register::MacInterruptStatus))
    }

    pub(super) fn quiesce(&self) -> QuiesceSnapshot {
        self.suppress_interrupts();
        let (mac_status_before, _) = self.service_mac_interrupts();
        // Sample and acknowledge causes before the ST/SR clear below. Only
        // causes observed after that stop linearization may use the cleanup
        // classification for TPS/RPS.
        let active_status = self.status();
        let active_legal = active_status & DmaStatus::W1C.bits();
        let mut w1c_samples = 0u32;
        if active_legal != 0 {
            self.acknowledge_causes(active_legal);
            w1c_samples = 1;
        }
        // Match Linux's legacy stop_all_dma ordering: RX first, then TX.
        let mut dma = self.read(Register::DmaControl);
        dma &= !DmaControl::RX_START.bits();
        self.write(Register::DmaControl, dma);
        dma &= !DmaControl::TX_START.bits();
        self.write(Register::DmaControl, dma);
        let mut mac = self.read(Register::MacControl);
        mac &= !(MacControl::TX_ENABLE.bits()
            | MacControl::RX_ENABLE.bits()
            | MacControl::LOOPBACK.bits());
        self.write(Register::MacControl, mac);

        let start = MonotonicInstant::now();
        let mut cleanup_legal = 0;
        loop {
            let before = self.status();
            let legal = before & DmaStatus::W1C.bits();
            cleanup_legal |= legal;
            if legal != 0 {
                self.acknowledge_causes(legal);
                w1c_samples = w1c_samples.saturating_add(1);
            }
            let status = self.status();
            let tx_process = (status & DmaStatus::TX_PROCESS_MASK.bits()) >> DMA_TX_PROCESS_SHIFT;
            let rx_process = (status & DmaStatus::RX_PROCESS_MASK.bits()) >> DMA_RX_PROCESS_SHIFT;
            let control = self.read(Register::DmaControl);
            let mac_control = self.read(Register::MacControl);
            let stopped = tx_process == 0
                && rx_process == 0
                && control & (DmaControl::TX_START.bits() | DmaControl::RX_START.bits()) == 0
                && mac_control & (MacControl::TX_ENABLE.bits() | MacControl::RX_ENABLE.bits()) == 0;
            if (stopped && status & DmaStatus::W1C.bits() == 0)
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
        status & DmaStatus::W1C.bits()
    } else {
        0
    }
}

fn mdio_command(clock_range: u32, phy: u8, register: u8, write: bool) -> u32 {
    assert!(clock_range & !MdioAddress::CLOCK_RANGE_MASK.bits() == 0);
    assert!(phy <= 0x1f && register <= 0x1f);
    MdioAddress::BUSY.bits()
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
            DmaControl::TX_STORE_FORWARD.bits()
                | DmaControl::RX_STORE_FORWARD.bits()
                | DmaControl::OPERATE_SECOND_FRAME.bits()
        },
        SelectedDmaMode::ThresholdTxStoreForwardRx => DmaControl::RX_STORE_FORWARD.bits(),
    }
}

const fn dma_mode_mask() -> u32 {
    DmaControl::TX_STORE_FORWARD.bits()
        | DmaControl::RX_STORE_FORWARD.bits()
        | DmaControl::OPERATE_SECOND_FRAME.bits()
        | DmaControl::TX_THRESHOLD_MASK.bits()
        | DmaControl::RX_THRESHOLD_MASK.bits()
}

const fn fifo_flow_control_expected(rx_fifo_bytes: Option<u32>) -> bool {
    matches!(rx_fifo_bytes, Some(bytes) if bytes >= 4096)
}

const fn configure_fifo_flow_control(control: u32, rx_fifo_bytes: Option<u32>) -> u32 {
    let control = control
        & !(DmaControl::FLOW_CONTROL_ENABLE.bits()
            | DmaControl::FLOW_ACTIVATION_MASK.bits()
            | DmaControl::FLOW_DEACTIVATION_MASK.bits());
    if fifo_flow_control_expected(rx_fifo_bytes) {
        // Linux DWMAC1000 selects full-minus-1K activation and
        // full-minus-2K deactivation for every exact FIFO >= 4 KiB.
        control | DmaControl::FLOW_CONTROL_ENABLE.bits() | 0x0000_0800
    } else {
        control
    }
}

const fn fifo_flow_control_enabled(control: u32) -> bool {
    control & DmaControl::FLOW_CONTROL_ENABLE.bits() != 0
}

const fn effective_mac_interrupt_mask(capabilities: Dwmac1000Capabilities) -> u32 {
    let mut mask = MacInterruptMask::LEGACY_REQUESTED.bits();
    // DWMAC1000 drops mask bits for absent optional interrupt sources on
    // readback. Derive the effective value from Register::DmaHardwareFeature rather
    // than a board-observed constant.
    if !capabilities.pcs {
        mask &= !(MacInterrupt::PCS_LINK.bits() | MacInterrupt::PCS_AUTO_NEGOTIATION.bits());
    }
    if !(capabilities.remote_wake || capabilities.magic_wake) {
        mask &= !MacInterrupt::POWER_MANAGEMENT.bits();
    }
    if !(capabilities.timestamp_v1 || capabilities.timestamp_v2) {
        mask &= !(1 << 9);
    }
    mask
}

const fn descriptor_rearm_valid(
    dma_control: u32,
    actual_rx: u32,
    actual_tx: u32,
    expected_rx: u32,
    expected_tx: u32,
) -> bool {
    dma_control & (DmaControl::RX_START.bits() | DmaControl::TX_START.bits()) == 0
        && actual_rx == expected_rx
        && actual_tx == expected_tx
}

const fn mac_flow_control(link: PhyLink) -> u32 {
    MacFlowControl::UNICAST_PAUSE.bits()
        | if link.full_duplex {
            MacFlowControl::PAUSE_TIME_MASK.bits()
        } else {
            0
        }
        | if link.rx_pause {
            MacFlowControl::RX_ENABLE.bits()
        } else {
            0
        }
        | if link.tx_pause {
            MacFlowControl::TX_ENABLE.bits()
        } else {
            0
        }
}

const fn mac_link_bits(speed_mbps: u32, full_duplex: bool) -> Result<u32, SysError> {
    let speed = match speed_mbps {
        1000 => 0,
        100 => MacControl::PORT_SELECT.bits() | MacControl::FAST_ETHERNET_SPEED.bits(),
        10 => MacControl::PORT_SELECT.bits(),
        _ => return Err(SysError::InvalidArgument),
    };
    Ok(speed
        | if full_duplex {
            MacControl::FULL_DUPLEX.bits()
        } else {
            0
        })
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
    pub(super) dma_current_tx_buffer: u32,
    pub(super) dma_current_rx_buffer: u32,
    pub(super) dma_missed_frame_counter: u32,
    pub(super) mac_control: u32,
    pub(super) mac_interrupt_mask: u32,
    pub(super) rgmii_status: u32,
    pub(super) mac_address_high: u32,
    pub(super) mac_address_low: u32,
    pub(super) frame_filter: u32,
    pub(super) mmc_rx_frame_count_gb: u32,
    pub(super) mmc_rx_unicast: u32,
    pub(super) mmc_rx_crc_error: u32,
    pub(super) mmc_rx_align_error: u32,
    pub(super) mmc_rx_run_error: u32,
    pub(super) mmc_rx_length_error: u32,
    pub(super) mmc_rx_fifo_overflow: u32,
    pub(super) mmc_rx_watchdog_error: u32,
}

const fn decode_capabilities(version: u32, hw_feature: u32) -> Dwmac1000Capabilities {
    Dwmac1000Capabilities {
        version,
        hw_feature,
        enhanced_descriptors: hw_feature & HardwareFeature::ENHANCED_DESCRIPTORS.bits() != 0,
        mii: hw_feature & HardwareFeature::MII.bits() != 0,
        gmii: hw_feature & HardwareFeature::GMII.bits() != 0,
        half_duplex: hw_feature & HardwareFeature::HALF_DUPLEX.bits() != 0,
        pcs: hw_feature & HardwareFeature::PCS.bits() != 0,
        mdio: hw_feature & HardwareFeature::MDIO.bits() != 0,
        remote_wake: hw_feature & HardwareFeature::REMOTE_WAKE.bits() != 0,
        magic_wake: hw_feature & HardwareFeature::MAGIC_WAKE.bits() != 0,
        rmon: hw_feature & HardwareFeature::MMC.bits() != 0,
        timestamp_v1: hw_feature & HardwareFeature::TIMESTAMP_V1.bits() != 0,
        timestamp_v2: hw_feature & HardwareFeature::TIMESTAMP_V2.bits() != 0,
        eee: hw_feature & HardwareFeature::EEE.bits() != 0,
        tx_checksum: hw_feature & HardwareFeature::TX_CHECKSUM.bits() != 0,
        rx_checksum_type1: hw_feature & HardwareFeature::RX_CHECKSUM_TYPE1.bits() != 0,
        rx_checksum_type2: hw_feature & HardwareFeature::RX_CHECKSUM_TYPE2.bits() != 0,
        rx_fifo_over_2048: hw_feature & HardwareFeature::RX_FIFO_OVER_2048.bits() != 0,
        rx_channels: (((hw_feature & HardwareFeature::RX_CHANNEL_MASK.bits()) >> 20) + 1) as u8,
        tx_channels: (((hw_feature & HardwareFeature::TX_CHANNEL_MASK.bits()) >> 22) + 1) as u8,
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn production_descriptor_rearm_requires_stopped_dma_and_exact_bases() {
        let rx = 0x0e80_0000;
        let tx = 0x0e80_0800;
        assert!(descriptor_rearm_valid(0, rx, tx, rx, tx));
        assert!(!descriptor_rearm_valid(
            DmaControl::RX_START.bits(),
            rx,
            tx,
            rx,
            tx
        ));
        assert!(!descriptor_rearm_valid(
            DmaControl::TX_START.bits(),
            rx,
            tx,
            rx,
            tx
        ));
        assert!(!descriptor_rearm_valid(0, rx + 32, tx, rx, tx));
        assert!(!descriptor_rearm_valid(0, rx, tx + 32, rx, tx));
    }

    #[kunit]
    fn gate2_admission_requires_linux_enhanced_descriptor_capability() {
        let base = HardwareFeature::MII.bits() | HardwareFeature::MDIO.bits();
        assert!(!decode_capabilities(0x37, base).supported_gate2());
        assert!(
            decode_capabilities(0x37, base | HardwareFeature::ENHANCED_DESCRIPTORS.bits())
                .supported_gate2()
        );
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
        assert_eq!(enabled & DmaControl::FLOW_DEACTIVATION_MASK.bits(), 0x800);
    }

    #[kunit]
    fn capability_decode_preserves_linux_channel_and_optional_feature_facts() {
        let capabilities = decode_capabilities(
            EXPECTED_CORE_VERSION as u32,
            HardwareFeature::MII.bits()
                | HardwareFeature::GMII.bits()
                | HardwareFeature::MDIO.bits()
                | HardwareFeature::MMC.bits()
                | HardwareFeature::TX_CHECKSUM.bits()
                | HardwareFeature::ENHANCED_DESCRIPTORS.bits()
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
        let base = HardwareFeature::MII.bits()
            | HardwareFeature::MDIO.bits()
            | HardwareFeature::REMOTE_WAKE.bits()
            | HardwareFeature::TIMESTAMP_V2.bits();
        assert_eq!(
            effective_mac_interrupt_mask(decode_capabilities(EXPECTED_CORE_VERSION as u32, base)),
            0x209
        );
        assert_eq!(
            effective_mac_interrupt_mask(decode_capabilities(
                EXPECTED_CORE_VERSION as u32,
                base | HardwareFeature::PCS.bits()
            )),
            MacInterruptMask::LEGACY_REQUESTED.bits()
        );
        assert_eq!(
            effective_mac_interrupt_mask(decode_capabilities(
                EXPECTED_CORE_VERSION as u32,
                HardwareFeature::MII.bits() | HardwareFeature::MDIO.bits()
            )),
            MacInterrupt::RGMII.bits()
        );
    }

    #[kunit]
    fn mdio_read_command_keeps_divider_phy_and_register_fields_separate() {
        let command = mdio_command(2 << MII_CLOCK_RANGE_SHIFT, 3, 2, false);
        assert_eq!(command & 1, MdioAddress::BUSY.bits());
        assert_eq!(
            command & MdioAddress::CLOCK_RANGE_MASK.bits(),
            2 << MII_CLOCK_RANGE_SHIFT
        );
        assert_eq!((command >> 6) & 0x1f, 2);
        assert_eq!((command >> 11) & 0x1f, 3);
    }

    #[kunit]
    fn quiesce_timeout_without_stop_does_not_claim_w1c_failure() {
        assert_eq!(quiesce_uncleared(DmaStatus::W1C.bits(), false), 0);
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
        assert!(Dwmac1000Regs::atds(
            DmaBusMode::ALTERNATE_DESCRIPTOR_SIZE.bits()
        ));
    }

    #[kunit]
    fn legacy_mac_link_bits_cover_only_clause_22_speeds() {
        assert_eq!(
            mac_link_bits(1000, true),
            Ok(MacControl::FULL_DUPLEX.bits())
        );
        assert_eq!(
            mac_link_bits(100, false),
            Ok(MacControl::PORT_SELECT.bits() | MacControl::FAST_ETHERNET_SPEED.bits())
        );
        assert_eq!(mac_link_bits(10, false), Ok(MacControl::PORT_SELECT.bits()));
        assert_eq!(mac_link_bits(2500, true), Err(SysError::InvalidArgument));
    }
}
