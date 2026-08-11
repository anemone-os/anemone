use crate::{mm::remap::IoRemap, prelude::*, time::MonotonicInstant};

// Fixed DWMAC 5.20 register facts are cross-checked against
// xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/{dwmac4.h,dwmac4_dma.h,
// hwif.h}.

mod offsets {
    pub const GMAC_CONFIG: usize = 0x0000;
    pub const GMAC_PACKET_FILTER: usize = 0x0008;
    pub const GMAC_RXQ_CTRL0: usize = 0x00a0;
    pub const GMAC_MDIO_ADDRESS: usize = 0x0200;
    pub const GMAC_MDIO_DATA: usize = 0x0204;
    pub const DMA_BUS_MODE: usize = 0x1000;
    pub const DMA_SYS_BUS_MODE: usize = 0x1004;
    pub const GMAC_INT_ENABLE: usize = 0x00b4;
    pub const GMAC_HW_FEATURE0: usize = 0x011c;
    pub const GMAC_HW_FEATURE1: usize = 0x0120;
    pub const GMAC_HW_FEATURE2: usize = 0x0124;
    pub const GMAC_HW_FEATURE3: usize = 0x0128;
    pub const GMAC4_VERSION: usize = 0x0110;
    pub const GMAC_ADDR_HIGH0: usize = 0x0300;
    pub const GMAC_ADDR_LOW0: usize = 0x0304;
    pub const MTL_CHANNEL0_TX_OPERATION: usize = 0x0d00;
    pub const MTL_CHANNEL0_RX_OPERATION: usize = 0x0d30;
    pub const MTL_RXQ_DMA_MAP0: usize = 0x0c30;
    pub const DMA_CHANNEL0_BASE: usize = 0x1100;
    pub const DMA_CHANNEL_INTERRUPT_ENABLE: usize = 0x34;
    pub const DMA_CHANNEL_TX_CONTROL: usize = 0x04;
    pub const DMA_CHANNEL_RX_CONTROL: usize = 0x08;
    pub const DMA_CHANNEL_TX_BASE_HIGH: usize = 0x10;
    pub const DMA_CHANNEL_TX_BASE_LOW: usize = 0x14;
    pub const DMA_CHANNEL_RX_BASE_HIGH: usize = 0x18;
    pub const DMA_CHANNEL_RX_BASE_LOW: usize = 0x1c;
    pub const DMA_CHANNEL_TX_TAIL: usize = 0x20;
    pub const DMA_CHANNEL_RX_TAIL: usize = 0x28;
    pub const DMA_CHANNEL_TX_RING_LENGTH: usize = 0x2c;
    pub const DMA_CHANNEL_RX_RING_LENGTH: usize = 0x30;
    pub const DMA_CHANNEL_STATUS: usize = 0x60;
    pub const DMA_CHANNEL_CURRENT_RX_DESC: usize = 0x4c;
    pub const DMA_CHANNEL_CURRENT_RX_BUFFER: usize = 0x5c;
    pub const DMA_CHANNEL0_TX_CONTROL: usize = DMA_CHANNEL0_BASE + DMA_CHANNEL_TX_CONTROL;
    pub const DMA_CHANNEL0_RX_CONTROL: usize = DMA_CHANNEL0_BASE + DMA_CHANNEL_RX_CONTROL;
    pub const DMA_CHANNEL0_TX_BASE_HIGH: usize = DMA_CHANNEL0_BASE + DMA_CHANNEL_TX_BASE_HIGH;
    pub const DMA_CHANNEL0_TX_BASE_LOW: usize = DMA_CHANNEL0_BASE + DMA_CHANNEL_TX_BASE_LOW;
    pub const DMA_CHANNEL0_RX_BASE_HIGH: usize = DMA_CHANNEL0_BASE + DMA_CHANNEL_RX_BASE_HIGH;
    pub const DMA_CHANNEL0_RX_BASE_LOW: usize = DMA_CHANNEL0_BASE + DMA_CHANNEL_RX_BASE_LOW;
    pub const DMA_CHANNEL0_TX_TAIL: usize = DMA_CHANNEL0_BASE + DMA_CHANNEL_TX_TAIL;
    pub const DMA_CHANNEL0_RX_TAIL: usize = DMA_CHANNEL0_BASE + DMA_CHANNEL_RX_TAIL;
    pub const DMA_CHANNEL0_TX_RING_LENGTH: usize = DMA_CHANNEL0_BASE + DMA_CHANNEL_TX_RING_LENGTH;
    pub const DMA_CHANNEL0_RX_RING_LENGTH: usize = DMA_CHANNEL0_BASE + DMA_CHANNEL_RX_RING_LENGTH;
    pub const DMA_CHANNEL0_CURRENT_RX_DESC: usize = DMA_CHANNEL0_BASE + DMA_CHANNEL_CURRENT_RX_DESC;
    pub const DMA_CHANNEL0_CURRENT_RX_BUFFER: usize =
        DMA_CHANNEL0_BASE + DMA_CHANNEL_CURRENT_RX_BUFFER;
    pub const DMA_CHANNEL0_INTERRUPT_ENABLE: usize =
        DMA_CHANNEL0_BASE + DMA_CHANNEL_INTERRUPT_ENABLE;
    pub const DMA_CHANNEL0_STATUS: usize = DMA_CHANNEL0_BASE + DMA_CHANNEL_STATUS;
}

const EXPECTED_DWMAC_VERSION: u8 = 0x52;
const GMAC_ADDR_HIGH_AE: u32 = 1 << 31;
const FIFO_DEPTH_UNIT_BYTES: usize = 128;

static_assert!(
    JH7110_GMAC_RESET_TIMEOUT_MS > 0,
    "JH7110_GMAC_RESET_TIMEOUT_MS must be non-zero"
);

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct GmacConfig: u32 {
        const CORE_INIT = (1 << 17) | (1 << 15) | (1 << 18) | (1 << 9) | (1 << 16);
        const SPEED_MASK = (1 << 14) | (1 << 15);
        const FULL_DUPLEX = 1 << 13;
        const TE = 1 << 1;
        const RE = 1;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct GmacPacketFilter: u32 {
        const HASH_OR_PERFECT = 1 << 10;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct GmacRxQueueControl: u32 {
        const QUEUE0_ENABLE_MASK = 0b11;
        const QUEUE0_DCB_ENABLE = 0b10;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct MtlRxDmaMap: u32 {
        const QUEUE0_CHANNEL_MASK = 0xf;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct MdioAddress: u32 {
        const BUSY = 1;
        const WRITE = 1 << 2;
        const READ = 3 << 2;
        const CLOCK_RANGE_MASK = 0xf << 8;
        const REGISTER_MASK = 0x1f << 16;
        const PHY_MASK = 0x1f << 21;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct GmacHwFeature0: u32 {
        const GMIISEL = 1 << 1;
        // Preserve feature bits that are not modeled by this driver.
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct GmacHwFeature1: u32 {
        const RX_FIFO_SIZE_MASK = 0x1f;
        const TX_FIFO_SIZE_MASK = 0x1f << 6;
        // Preserve feature bits that are not modeled by this driver.
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct GmacHwFeature2: u32 {
        const RX_QUEUE_MASK = 0xf;
        const TX_QUEUE_MASK = 0xf << 6;
        // Preserve feature bits that are not modeled by this driver.
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct DmaSystemBusMode: u32 {
        const FIXED_BURST = 1;
        const BURST_LENGTH_MASK = 0x7f << 1;
        const ADDRESS_ALIGNED = 1 << 12;
        const WRITE_OSR_MASK = 0xf << 24;
        const READ_OSR_MASK = 0xf << 16;
        const EXTENDED_ADDRESS = 1 << 11;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct DmaBusMode: u32 {
        const SOFTWARE_RESET = 1;
        // StarFive's Linux glue enables DWMAC coherent/cacheable bus
        // transactions through this DMA master hint.
        const DCHE = 1 << 19;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct DmaTxControl: u32 {
        const PBL_MASK = 0x3f << 16;
        const OPERATE_SECOND_PACKET = 1 << 4;
        const START = 1;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct DmaRxControl: u32 {
        const PBL_MASK = 0x3f << 16;
        const BUFFER_SIZE_MASK = 0x3fff << 1;
        const START = 1;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct MtlTxOperation: u32 {
        const FIFO_SIZE_MASK = 0x1ff << 16;
        const QUEUE_ENABLE_MASK = 0b11 << 2;
        const QUEUE_ENABLE_DCB = 1 << 3;
        const THRESHOLD_MASK = 0b111 << 4;
        const THRESHOLD_64 = 1 << 4;
        const STORE_AND_FORWARD = 1 << 1;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct MtlRxOperation: u32 {
        const FIFO_SIZE_MASK = 0x3ff << 20;
        const THRESHOLD_MASK = 0b11 << 3;
        const STORE_AND_FORWARD = 1 << 5;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct DmaInterruptEnable: u32 {
        // DWMAC 4.10 and later moved the summary enables down one bit.
        const NORMAL = 1 << 15;
        const ABNORMAL = 1 << 14;
        const CDE = 1 << 13;
        const FBE = 1 << 12;
        const RX = 1 << 6;
        const TX = 1;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct DmaStatus: u32 {
        const RX_ERROR_BITS = 0b111 << 19;
        const TX_ERROR_BITS = 0b111 << 16;
        const NORMAL = 1 << 15;
        const ABNORMAL = 1 << 14;
        const CDE = 1 << 13;
        const FBE = 1 << 12;
        const ERI = 1 << 11;
        const ETI = 1 << 10;
        const RWT = 1 << 9;
        const RPS = 1 << 8;
        const RBU = 1 << 7;
        const RX = 1 << 6;
        const TBU = 1 << 2;
        const TPS = 1 << 1;
        const TX = 1;
        const _ = !0;
    }
}

pub(super) const DWMAC_DMA_ADDRESS_BITS: u32 = 40;
const DWMAC_DMA_ADDRESS_ENCODING_40: u32 = 1;
// JH7110's 125 MHz stmmaceth CSR clock uses Linux's 100-150 MHz divider.
const MDIO_CLOCK_RANGE: u32 = 1;
const DWMAC_DMA_STATUS_W1C: DmaStatus = DmaStatus::from_bits_retain(
    DmaStatus::RX_ERROR_BITS.bits()
        | DmaStatus::TX_ERROR_BITS.bits()
        | DmaStatus::NORMAL.bits()
        | DmaStatus::ABNORMAL.bits()
        | DmaStatus::CDE.bits()
        | DmaStatus::FBE.bits()
        | DmaStatus::ERI.bits()
        | DmaStatus::ETI.bits()
        | DmaStatus::RWT.bits()
        | DmaStatus::RPS.bits()
        | DmaStatus::RBU.bits()
        | DmaStatus::RX.bits()
        | DmaStatus::TBU.bits()
        | DmaStatus::TPS.bits()
        | DmaStatus::TX.bits(),
);

/// Immutable probe-time snapshot of admitted hardware capabilities. These
/// fields support construction and diagnostics; the registers remain the
/// authoritative live hardware state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GmacCapabilities {
    pub(super) version: u8,
    pub(super) hw_feature0: u32,
    pub(super) hw_feature1: u32,
    pub(super) hw_feature2: u32,
    pub(super) hw_feature3: u32,
    pub(super) rx_queues: u8,
    pub(super) tx_queues: u8,
    pub(super) dma_address_bits: u32,
}

impl GmacCapabilities {
    pub(super) fn rx_fifo_depth(&self) -> usize {
        fifo_depth_bytes(self.hw_feature1, GmacHwFeature1::RX_FIFO_SIZE_MASK, 0)
    }

    pub(super) fn tx_fifo_depth(&self) -> usize {
        fifo_depth_bytes(self.hw_feature1, GmacHwFeature1::TX_FIFO_SIZE_MASK, 6)
    }
}

fn fifo_depth_bytes(feature1: u32, mask: GmacHwFeature1, shift: u32) -> usize {
    let encoded = (feature1 & mask.bits()) >> shift;
    FIFO_DEPTH_UNIT_BYTES << encoded
}

/// Bounds-checked volatile view over one JH7110 GMAC MMIO resource.
pub(super) struct GmacRegs {
    remap: IoRemap,
}

impl GmacRegs {
    pub(super) const BASELINE_MAPPING_LEN: usize = offsets::DMA_CHANNEL0_STATUS + 4;

    pub(super) fn new(remap: IoRemap) -> Result<Self, SysError> {
        if remap.size() < Self::BASELINE_MAPPING_LEN as u64 {
            kerrln!(
                "jh7110-gmac: MMIO mapping too short base={:#x} size={:#x} required={:#x}",
                remap.phys_base().get(),
                remap.size(),
                Self::BASELINE_MAPPING_LEN
            );
            return Err(SysError::DriverIncompatible);
        }
        Ok(Self { remap })
    }

    pub(super) fn phys_base(&self) -> PhysAddr {
        self.remap.phys_base()
    }

    pub(super) fn size(&self) -> usize {
        self.remap.size() as usize
    }

    fn ptr_at(&self, offset: usize) -> *mut u32 {
        let end = offset
            .checked_add(core::mem::size_of::<u32>())
            .expect("JH7110 GMAC MMIO offset overflow");
        assert!(
            end <= self.size(),
            "JH7110 GMAC MMIO access outside mapping"
        );
        assert!(offset.is_multiple_of(core::mem::align_of::<u32>()));
        unsafe { self.remap.as_ptr().as_ptr().cast::<u8>().add(offset).cast() }
    }

    fn read(&self, offset: usize) -> u32 {
        mmio_write_read_barrier();
        let value = unsafe { core::ptr::read_volatile(self.ptr_at(offset)) };
        mmio_read_barrier();
        value
    }

    fn write(&self, offset: usize, value: u32) {
        mmio_write_barrier();
        unsafe { core::ptr::write_volatile(self.ptr_at(offset), value) }
    }

    /// Reset the internal DWMAC DMA before programming bus or channel state.
    /// The external reset controller does not replace this self-clearing DMA
    /// reset required by the DWMAC initialization protocol.
    pub(super) fn reset_dma(&self) -> Result<(), SysError> {
        let start = MonotonicInstant::now();
        let timeout = Duration::from_millis(JH7110_GMAC_RESET_TIMEOUT_MS);
        let mut mode = DmaBusMode::from_bits_retain(self.read(offsets::DMA_BUS_MODE));
        mode.insert(DmaBusMode::SOFTWARE_RESET);
        self.write(offsets::DMA_BUS_MODE, mode.bits());

        loop {
            let raw = self.read(offsets::DMA_BUS_MODE);
            if !DmaBusMode::from_bits_retain(raw).contains(DmaBusMode::SOFTWARE_RESET) {
                return Ok(());
            }
            if start.elapsed() >= timeout {
                kerrln!(
                    "jh7110-gmac: internal DMA reset timeout base={:#x} timeout-ms={} dma_bus={:#x}",
                    self.phys_base().get(),
                    JH7110_GMAC_RESET_TIMEOUT_MS,
                    raw,
                );
                return Err(SysError::Timeout);
            }
            core::hint::spin_loop();
        }
    }

    pub(super) fn capabilities(&self) -> Result<GmacCapabilities, SysError> {
        let version_raw = self.read(offsets::GMAC4_VERSION);
        let version = version_raw as u8;
        if version != EXPECTED_DWMAC_VERSION {
            kerrln!(
                "jh7110-gmac: unexpected DWMAC version base={:#x} offset={:#x} raw={:#x} got={:#x} expected={:#x}",
                self.phys_base().get(),
                offsets::GMAC4_VERSION,
                version_raw,
                version,
                EXPECTED_DWMAC_VERSION
            );
            return Err(SysError::DriverIncompatible);
        }

        // DWMAC4 capability offsets are only meaningful after the family
        // version has been admitted; avoid probing unrelated register layouts.
        let hw_feature0 = self.read(offsets::GMAC_HW_FEATURE0);
        let hw_feature1 = self.read(offsets::GMAC_HW_FEATURE1);
        let hw_feature2 = self.read(offsets::GMAC_HW_FEATURE2);
        let hw_feature3 = self.read(offsets::GMAC_HW_FEATURE3);
        let hw_feature0_flags = GmacHwFeature0::from_bits_retain(hw_feature0);
        let hw_feature2_flags = GmacHwFeature2::from_bits_retain(hw_feature2);
        let dma_address_encoding = (hw_feature1 >> 14) & 0x3;
        let dma_address_bits = match dma_address_encoding {
            0 => 32,
            DWMAC_DMA_ADDRESS_ENCODING_40 => DWMAC_DMA_ADDRESS_BITS,
            2 => 48,
            _ => 32,
        };
        if dma_address_bits != DWMAC_DMA_ADDRESS_BITS {
            kerrln!(
                "jh7110-gmac: unsupported DWMAC DMA address width base={:#x} feature1={:#x} encoding={:#x} bits={}",
                self.phys_base().get(),
                hw_feature1,
                dma_address_encoding,
                dma_address_bits
            );
            return Err(SysError::DriverIncompatible);
        }

        if !hw_feature0_flags.contains(GmacHwFeature0::GMIISEL) {
            kerrln!(
                "jh7110-gmac: GMIISEL capability missing base={:#x} feature0={:#x} feature1={:#x} feature2={:#x} feature3={:#x} required-bit={:#x}",
                self.phys_base().get(),
                hw_feature0,
                hw_feature1,
                hw_feature2,
                hw_feature3,
                GmacHwFeature0::GMIISEL.bits()
            );
            return Err(SysError::DriverIncompatible);
        }
        let rx_queues =
            ((hw_feature2_flags.bits() & GmacHwFeature2::RX_QUEUE_MASK.bits()) + 1) as u8;
        let tx_queues =
            (((hw_feature2_flags.bits() & GmacHwFeature2::TX_QUEUE_MASK.bits()) >> 6) + 1) as u8;
        Ok(GmacCapabilities {
            version,
            hw_feature0,
            hw_feature1,
            hw_feature2,
            hw_feature3,
            rx_queues,
            tx_queues,
            dma_address_bits,
        })
    }

    pub(super) fn mdio_read(&self, phy: u8, register: u8) -> Result<u16, SysError> {
        assert!(phy <= 0x1f && register <= 0x1f);
        self.wait_for_mdio_idle()?;
        self.write(offsets::GMAC_MDIO_DATA, 0);
        self.write(
            offsets::GMAC_MDIO_ADDRESS,
            MdioAddress::BUSY.bits()
                | MdioAddress::READ.bits()
                | (MDIO_CLOCK_RANGE << 8)
                | ((register as u32) << 16)
                | ((phy as u32) << 21),
        );
        self.wait_for_mdio_idle()?;
        Ok(self.read(offsets::GMAC_MDIO_DATA) as u16)
    }

    pub(super) fn mdio_write(&self, phy: u8, register: u8, value: u16) -> Result<(), SysError> {
        assert!(phy <= 0x1f && register <= 0x1f);
        self.wait_for_mdio_idle()?;
        self.write(offsets::GMAC_MDIO_DATA, value as u32);
        self.write(
            offsets::GMAC_MDIO_ADDRESS,
            MdioAddress::BUSY.bits()
                | MdioAddress::WRITE.bits()
                | (MDIO_CLOCK_RANGE << 8)
                | ((register as u32) << 16)
                | ((phy as u32) << 21),
        );
        self.wait_for_mdio_idle()
    }

    fn wait_for_mdio_idle(&self) -> Result<(), SysError> {
        let start = MonotonicInstant::now();
        let timeout = Duration::from_millis(JH7110_GMAC_RESET_TIMEOUT_MS);
        loop {
            let address = MdioAddress::from_bits_retain(self.read(offsets::GMAC_MDIO_ADDRESS));
            if !address.contains(MdioAddress::BUSY) {
                return Ok(());
            }
            if start.elapsed() >= timeout {
                kerrln!(
                    "jh7110-gmac: MDIO transaction timeout base={:#x} address={:#x}",
                    self.phys_base().get(),
                    address.bits(),
                );
                return Err(SysError::Timeout);
            }
            core::hint::spin_loop();
        }
    }

    pub(super) fn configure_stopped_rings(
        &self,
        rx_desc_phys: u64,
        tx_desc_phys: u64,
        rx_tail_phys: u64,
        tx_tail_phys: u64,
        ring_size: usize,
        frame_capacity: usize,
        rx_pbl: u8,
        tx_pbl: u8,
        fixed_burst: bool,
        axi_write_requests: u8,
        axi_read_requests: u8,
        axi_burst_map: u8,
    ) -> Result<(), SysError> {
        let tx_control = self.read(offsets::DMA_CHANNEL0_TX_CONTROL);
        let rx_control = self.read(offsets::DMA_CHANNEL0_RX_CONTROL);
        if DmaTxControl::from_bits_retain(tx_control).contains(DmaTxControl::START)
            || DmaRxControl::from_bits_retain(rx_control).contains(DmaRxControl::START)
        {
            kerrln!(
                "jh7110-gmac: refusing ring setup while DMA channel is running base={:#x} tx_ctrl={:#x} rx_ctrl={:#x}",
                self.phys_base().get(),
                tx_control,
                rx_control
            );
            return Err(SysError::Busy);
        }
        assert!(ring_size.is_power_of_two() && ring_size >= 2);
        assert!(ring_size <= u32::MAX as usize);
        assert!(frame_capacity <= 0x3fff);
        assert!((1..=0x3f).contains(&rx_pbl));
        assert!((1..=0x3f).contains(&tx_pbl));
        assert!(axi_write_requests <= 0xf);
        assert!(axi_read_requests <= 0xf);
        assert!(axi_burst_map <= 0x7f);
        let mut sys_bus_mode =
            DmaSystemBusMode::from_bits_retain(self.read(offsets::DMA_SYS_BUS_MODE));
        sys_bus_mode.remove(
            DmaSystemBusMode::FIXED_BURST
                | DmaSystemBusMode::BURST_LENGTH_MASK
                | DmaSystemBusMode::WRITE_OSR_MASK
                | DmaSystemBusMode::READ_OSR_MASK,
        );
        if fixed_burst {
            sys_bus_mode.insert(DmaSystemBusMode::FIXED_BURST);
        }
        sys_bus_mode.insert(DmaSystemBusMode::EXTENDED_ADDRESS | DmaSystemBusMode::ADDRESS_ALIGNED);
        self.write(
            offsets::DMA_SYS_BUS_MODE,
            sys_bus_mode.bits()
                | ((axi_write_requests as u32) << 24)
                | ((axi_read_requests as u32) << 16)
                | ((axi_burst_map as u32) << 1),
        );
        let mut dma_bus_mode = DmaBusMode::from_bits_retain(self.read(offsets::DMA_BUS_MODE));
        dma_bus_mode.insert(DmaBusMode::DCHE);
        self.write(offsets::DMA_BUS_MODE, dma_bus_mode.bits());
        let dma_bus_mode = self.read(offsets::DMA_BUS_MODE);
        if !DmaBusMode::from_bits_retain(dma_bus_mode).contains(DmaBusMode::DCHE) {
            // Linux programs DCHE without a readback contract. This register
            // may post writes or expose only the reset-visible value on this
            // integration, so a zero readback is diagnostic rather than a
            // probe failure; DMA coherence is established by the board
            // contract and the ring ownership fences.
            kwarningln!(
                "jh7110-gmac: DMA descriptor cache enable not observable after write base={:#x} dma_bus={:#x}",
                self.phys_base().get(),
                dma_bus_mode,
            );
        }
        self.write(
            offsets::DMA_CHANNEL0_TX_BASE_HIGH,
            (tx_desc_phys >> 32) as u32,
        );
        self.write(offsets::DMA_CHANNEL0_TX_BASE_LOW, tx_desc_phys as u32);
        self.write(
            offsets::DMA_CHANNEL0_RX_BASE_HIGH,
            (rx_desc_phys >> 32) as u32,
        );
        self.write(offsets::DMA_CHANNEL0_RX_BASE_LOW, rx_desc_phys as u32);
        self.write(offsets::DMA_CHANNEL0_TX_RING_LENGTH, (ring_size - 1) as u32);
        self.write(offsets::DMA_CHANNEL0_RX_RING_LENGTH, (ring_size - 1) as u32);
        let mut tx_control = DmaTxControl::from_bits_retain(tx_control);
        tx_control.remove(DmaTxControl::PBL_MASK);
        tx_control.insert(DmaTxControl::OPERATE_SECOND_PACKET);
        self.write(
            offsets::DMA_CHANNEL0_TX_CONTROL,
            tx_control.bits() | ((tx_pbl as u32) << 16),
        );
        let mut rx_control = DmaRxControl::from_bits_retain(rx_control);
        rx_control.remove(DmaRxControl::PBL_MASK | DmaRxControl::BUFFER_SIZE_MASK);
        self.write(
            offsets::DMA_CHANNEL0_RX_CONTROL,
            rx_control.bits() | ((rx_pbl as u32) << 16) | ((frame_capacity as u32) << 1),
        );
        self.write(offsets::DMA_CHANNEL0_TX_TAIL, tx_tail_phys as u32);
        self.write(offsets::DMA_CHANNEL0_RX_TAIL, rx_tail_phys as u32);
        Ok(())
    }

    pub(super) fn configure_single_queue_mac(
        &self,
        mac: [u8; 6],
        rx_fifo_depth: usize,
        tx_fifo_depth: usize,
        force_thresh_dma_mode: bool,
    ) {
        assert!(rx_fifo_depth >= 256 && rx_fifo_depth.is_multiple_of(256));
        assert!(tx_fifo_depth >= 256 && tx_fifo_depth.is_multiple_of(256));
        let rx_fifo_size = rx_fifo_depth / 256 - 1;
        let tx_fifo_size = tx_fifo_depth / 256 - 1;
        assert!(rx_fifo_size <= 0x3ff);
        assert!(tx_fifo_size <= 0x1ff);
        self.write(
            offsets::GMAC_ADDR_HIGH0,
            GMAC_ADDR_HIGH_AE | ((mac[5] as u32) << 8) | mac[4] as u32,
        );
        self.write(
            offsets::GMAC_ADDR_LOW0,
            ((mac[3] as u32) << 24)
                | ((mac[2] as u32) << 16)
                | ((mac[1] as u32) << 8)
                | mac[0] as u32,
        );
        let mut filter = GmacPacketFilter::from_bits_retain(self.read(offsets::GMAC_PACKET_FILTER));
        filter.insert(GmacPacketFilter::HASH_OR_PERFECT);
        self.write(offsets::GMAC_PACKET_FILTER, filter.bits());

        // DWMAC's MTL routing and queue-enable registers are programmed after
        // the MAC core defaults, matching Linux stmmac_mtl_configuration().
        let mut config = GmacConfig::from_bits_retain(self.read(offsets::GMAC_CONFIG));
        config.insert(GmacConfig::CORE_INIT);
        self.write(offsets::GMAC_CONFIG, config.bits());

        let mut rx_dma_map = MtlRxDmaMap::from_bits_retain(self.read(offsets::MTL_RXQ_DMA_MAP0));
        rx_dma_map.remove(MtlRxDmaMap::QUEUE0_CHANNEL_MASK);
        self.write(offsets::MTL_RXQ_DMA_MAP0, rx_dma_map.bits());
        self.enable_single_rx_queue();

        let mut tx_operation =
            MtlTxOperation::from_bits_retain(self.read(offsets::MTL_CHANNEL0_TX_OPERATION));
        tx_operation.remove(
            MtlTxOperation::FIFO_SIZE_MASK
                | MtlTxOperation::QUEUE_ENABLE_MASK
                | MtlTxOperation::THRESHOLD_MASK,
        );
        tx_operation.insert(MtlTxOperation::QUEUE_ENABLE_DCB);
        if force_thresh_dma_mode {
            tx_operation.insert(MtlTxOperation::THRESHOLD_64);
        } else {
            tx_operation.insert(MtlTxOperation::STORE_AND_FORWARD);
        }
        self.write(
            offsets::MTL_CHANNEL0_TX_OPERATION,
            tx_operation.bits() | ((tx_fifo_size as u32) << 16),
        );
        let mut rx_operation =
            MtlRxOperation::from_bits_retain(self.read(offsets::MTL_CHANNEL0_RX_OPERATION));
        rx_operation.remove(MtlRxOperation::FIFO_SIZE_MASK | MtlRxOperation::THRESHOLD_MASK);
        if force_thresh_dma_mode {
            // RTC=0 is the DWMAC 64-byte receive threshold used by Linux's
            // `tc = 64` default when force-thresh mode is selected.
        } else {
            rx_operation.insert(MtlRxOperation::STORE_AND_FORWARD);
        }
        self.write(
            offsets::MTL_CHANNEL0_RX_OPERATION,
            rx_operation.bits() | ((rx_fifo_size as u32) << 20),
        );
    }

    /// Disable device causes without releasing the MMIO, DMA or IRQ backing.
    pub(super) fn disable_device_interrupts(&self) {
        self.write(offsets::GMAC_INT_ENABLE, 0);
        self.write(offsets::DMA_CHANNEL0_INTERRUPT_ENABLE, 0);
    }

    /// DMA channel status is write-one-to-clear; never use read-modify-write.
    pub(super) fn acknowledge_dma_causes(&self) {
        let status = DmaStatus::from_bits_retain(self.read(offsets::DMA_CHANNEL0_STATUS));
        self.write(
            offsets::DMA_CHANNEL0_STATUS,
            (status & DWMAC_DMA_STATUS_W1C).bits(),
        );
    }

    pub(super) fn take_enabled_dma_causes(&self) -> u32 {
        let enabled =
            DmaInterruptEnable::from_bits_retain(self.read(offsets::DMA_CHANNEL0_INTERRUPT_ENABLE));
        let status = DmaStatus::from_bits_retain(self.read(offsets::DMA_CHANNEL0_STATUS));
        let causes = status & acknowledged_status_mask(enabled) & DWMAC_DMA_STATUS_W1C;
        if !causes.is_empty() {
            // W1C must happen before the IRQ flow completes the controller.
            self.write(offsets::DMA_CHANNEL0_STATUS, causes.bits());
        }
        causes.bits()
    }

    pub(super) fn enable_device_interrupts(&self, mac_mask: u32, dma_mask: u32) {
        self.write(offsets::GMAC_INT_ENABLE, mac_mask);
        self.write(offsets::DMA_CHANNEL0_INTERRUPT_ENABLE, dma_mask);
    }

    pub(super) fn enable_dma_interrupts(&self) {
        let enabled = DmaInterruptEnable::NORMAL
            | DmaInterruptEnable::ABNORMAL
            | DmaInterruptEnable::CDE
            | DmaInterruptEnable::FBE
            | DmaInterruptEnable::RX
            | DmaInterruptEnable::TX;
        self.enable_device_interrupts(0, enabled.bits());
    }

    pub(super) fn configure_link(&self, speed_mbps: u32, full_duplex: bool) {
        let mut config = GmacConfig::from_bits_retain(self.read(offsets::GMAC_CONFIG));
        config.remove(GmacConfig::SPEED_MASK);
        match speed_mbps {
            1000 => {},
            100 => config.insert(GmacConfig::SPEED_MASK),
            10 => config.insert(GmacConfig::from_bits_retain(1 << 15)),
            _ => panic!("unsupported JH7110 GMAC link speed"),
        }
        config.set(GmacConfig::FULL_DUPLEX, full_duplex);
        self.write(offsets::GMAC_CONFIG, config.bits());
    }

    pub(super) fn start_dma(&self) {
        let mut config = GmacConfig::from_bits_retain(self.read(offsets::GMAC_CONFIG));
        config.insert(GmacConfig::TE | GmacConfig::RE);
        self.write(offsets::GMAC_CONFIG, config.bits());
        let mut rx = DmaRxControl::from_bits_retain(self.read(offsets::DMA_CHANNEL0_RX_CONTROL));
        rx.insert(DmaRxControl::START);
        self.write(offsets::DMA_CHANNEL0_RX_CONTROL, rx.bits());
        let mut tx = DmaTxControl::from_bits_retain(self.read(offsets::DMA_CHANNEL0_TX_CONTROL));
        tx.insert(DmaTxControl::START);
        self.write(offsets::DMA_CHANNEL0_TX_CONTROL, tx.bits());
    }

    fn enable_single_rx_queue(&self) {
        let mut rx_queue = GmacRxQueueControl::from_bits_retain(self.read(offsets::GMAC_RXQ_CTRL0));
        rx_queue.remove(GmacRxQueueControl::QUEUE0_ENABLE_MASK);
        rx_queue.insert(GmacRxQueueControl::QUEUE0_DCB_ENABLE);
        self.write(offsets::GMAC_RXQ_CTRL0, rx_queue.bits());
    }

    pub(super) fn stop_dma(&self) {
        let mut config = GmacConfig::from_bits_retain(self.read(offsets::GMAC_CONFIG));
        config.remove(GmacConfig::TE | GmacConfig::RE);
        self.write(offsets::GMAC_CONFIG, config.bits());
        let mut tx = DmaTxControl::from_bits_retain(self.read(offsets::DMA_CHANNEL0_TX_CONTROL));
        tx.remove(DmaTxControl::START);
        self.write(offsets::DMA_CHANNEL0_TX_CONTROL, tx.bits());
        let mut rx = DmaRxControl::from_bits_retain(self.read(offsets::DMA_CHANNEL0_RX_CONTROL));
        rx.remove(DmaRxControl::START);
        self.write(offsets::DMA_CHANNEL0_RX_CONTROL, rx.bits());
    }

    pub(super) fn update_tx_tail(&self, tail: u32) {
        self.write(offsets::DMA_CHANNEL0_TX_TAIL, tail);
    }

    pub(super) fn update_rx_tail(&self, tail: u32) {
        self.write(offsets::DMA_CHANNEL0_RX_TAIL, tail);
    }
}

fn acknowledged_status_mask(enabled: DmaInterruptEnable) -> DmaStatus {
    let mut causes = DmaStatus::empty();
    if enabled.contains(DmaInterruptEnable::NORMAL) {
        causes.insert(DmaStatus::NORMAL | DmaStatus::RX | DmaStatus::TX);
    }
    if enabled.contains(DmaInterruptEnable::ABNORMAL) {
        causes.insert(
            DmaStatus::RX_ERROR_BITS
                | DmaStatus::TX_ERROR_BITS
                | DmaStatus::ABNORMAL
                | DmaStatus::ERI
                | DmaStatus::ETI
                | DmaStatus::RWT
                | DmaStatus::RPS
                | DmaStatus::RBU
                | DmaStatus::TBU
                | DmaStatus::TPS,
        );
    }
    if enabled.contains(DmaInterruptEnable::CDE) {
        causes.insert(DmaStatus::CDE);
    }
    if enabled.contains(DmaInterruptEnable::FBE) {
        causes.insert(DmaStatus::FBE);
    }
    causes
}

#[inline(always)]
fn mmio_write_barrier() {
    #[cfg(target_arch = "riscv64")]
    unsafe {
        core::arch::asm!("fence w,o", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "riscv64"))]
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}

/// Order prior MMIO outputs before a following MMIO input. This is device I/O
/// ordering only; DMA coherency remains a property of the allocated backing.
#[inline(always)]
fn mmio_write_read_barrier() {
    #[cfg(target_arch = "riscv64")]
    unsafe {
        core::arch::asm!("fence o,i", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "riscv64"))]
    unreachable!("JH7110 GMAC MMIO ordering requires RISC-V");
}

#[inline(always)]
fn mmio_read_barrier() {
    #[cfg(target_arch = "riscv64")]
    unsafe {
        core::arch::asm!("fence i,ir", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "riscv64"))]
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn fifo_depth_decoding_uses_synthesized_feature_encoding() {
        let capabilities = GmacCapabilities {
            version: EXPECTED_DWMAC_VERSION,
            hw_feature0: GmacHwFeature0::GMIISEL.bits(),
            hw_feature1: 0x0984_5904,
            hw_feature2: 0,
            hw_feature3: 0,
            rx_queues: 1,
            tx_queues: 1,
            dma_address_bits: DWMAC_DMA_ADDRESS_BITS,
        };
        assert_eq!(capabilities.rx_fifo_depth(), 2048);
        assert_eq!(capabilities.tx_fifo_depth(), 2048);
    }

    #[kunit]
    fn dma_irq_enable_maps_only_acknowledgeable_w1c_causes() {
        let normal = acknowledged_status_mask(
            DmaInterruptEnable::NORMAL | DmaInterruptEnable::RX | DmaInterruptEnable::TX,
        );
        assert!(normal.contains(DmaStatus::NORMAL | DmaStatus::RX | DmaStatus::TX));
        assert!(!normal.intersects(DmaStatus::ABNORMAL | DmaStatus::FBE));

        let abnormal =
            acknowledged_status_mask(DmaInterruptEnable::ABNORMAL | DmaInterruptEnable::FBE);
        assert!(abnormal.contains(
            DmaStatus::RX_ERROR_BITS
                | DmaStatus::TX_ERROR_BITS
                | DmaStatus::ABNORMAL
                | DmaStatus::RBU
                | DmaStatus::FBE
        ));
        assert!(!abnormal.intersects(DmaStatus::NORMAL | DmaStatus::RX | DmaStatus::TX));
        assert_eq!(
            (normal | abnormal) & !DWMAC_DMA_STATUS_W1C,
            DmaStatus::empty()
        );
    }
}
