use crate::{mm::remap::IoRemap, prelude::*};

// Fixed DWMAC 5.20 register facts are cross-checked against
// xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/{dwmac4.h,dwmac4_dma.h,
// hwif.h}.

mod offsets {
    pub const DMA_SYS_BUS_MODE: usize = 0x1004;
    pub const GMAC_INT_ENABLE: usize = 0x00b4;
    pub const GMAC_HW_FEATURE0: usize = 0x011c;
    pub const GMAC_HW_FEATURE1: usize = 0x0120;
    pub const GMAC_HW_FEATURE2: usize = 0x0124;
    pub const GMAC_HW_FEATURE3: usize = 0x0128;
    pub const GMAC4_VERSION: usize = 0x0110;
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
    pub const DMA_CHANNEL0_INTERRUPT_ENABLE: usize =
        DMA_CHANNEL0_BASE + DMA_CHANNEL_INTERRUPT_ENABLE;
    pub const DMA_CHANNEL0_STATUS: usize = DMA_CHANNEL0_BASE + DMA_CHANNEL_STATUS;
}

const EXPECTED_DWMAC_VERSION: u8 = 0x52;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct GmacHwFeature0: u32 {
        const GMIISEL = 1 << 1;
        // Preserve feature bits that are not modeled by Gate 0.
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct GmacHwFeature2: u32 {
        const RX_QUEUE_MASK = 0xf;
        const TX_QUEUE_MASK = 0xf << 6;
        // Preserve feature bits that are not modeled by Gate 0.
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct DmaSystemBusMode: u32 {
        const EXTENDED_ADDRESS = 1 << 11;
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

const DWMAC_DMA_ADDRESS_BITS: u32 = 40;
const DWMAC_DMA_ADDRESS_ENCODING_40: u32 = 1;
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
        let value = unsafe { core::ptr::read_volatile(self.ptr_at(offset)) };
        mmio_read_barrier();
        value
    }

    fn write(&self, offset: usize, value: u32) {
        mmio_write_barrier();
        unsafe { core::ptr::write_volatile(self.ptr_at(offset), value) }
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

    pub(super) fn configure_stopped_rings(
        &self,
        rx_desc_phys: u64,
        tx_desc_phys: u64,
        rx_tail_phys: u64,
        tx_tail_phys: u64,
        ring_size: usize,
        frame_capacity: usize,
    ) -> Result<(), SysError> {
        let tx_control = self.read(offsets::DMA_CHANNEL0_TX_CONTROL);
        let rx_control = self.read(offsets::DMA_CHANNEL0_RX_CONTROL);
        if tx_control & 1 != 0 || rx_control & 1 != 0 {
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
        let mut sys_bus_mode =
            DmaSystemBusMode::from_bits_retain(self.read(offsets::DMA_SYS_BUS_MODE));
        sys_bus_mode.insert(DmaSystemBusMode::EXTENDED_ADDRESS);
        self.write(offsets::DMA_SYS_BUS_MODE, sys_bus_mode.bits());
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
        let mut rx_control = rx_control & !(0x3fff << 1);
        rx_control |= (frame_capacity as u32) << 1;
        self.write(offsets::DMA_CHANNEL0_RX_CONTROL, rx_control);
        self.write(offsets::DMA_CHANNEL0_TX_TAIL, tx_tail_phys as u32);
        self.write(offsets::DMA_CHANNEL0_RX_TAIL, rx_tail_phys as u32);
        Ok(())
    }

    /// Gate 0 leaves all device-side causes disabled. Gate 1 may replace this
    /// with the same owner-local operation after rings and handler context are
    /// ready; no IRQ is requested while this baseline is in use.
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
    fn dwmac_5_20_capability_constants_cover_single_queue_path() {
        assert_eq!(offsets::GMAC4_VERSION, 0x110);
        assert_eq!(EXPECTED_DWMAC_VERSION, 0x52);
        assert_eq!(offsets::DMA_CHANNEL0_STATUS, 0x1160);
        assert!(GmacRegs::BASELINE_MAPPING_LEN > offsets::GMAC_HW_FEATURE3);
        assert_eq!(GmacHwFeature0::GMIISEL.bits(), 1 << 1);
        assert_eq!(GmacHwFeature2::RX_QUEUE_MASK.bits(), 0xf);
        assert_eq!(GmacHwFeature2::TX_QUEUE_MASK.bits(), 0xf << 6);
        assert_eq!(DWMAC_DMA_ADDRESS_ENCODING_40, 1);
        assert_eq!(DWMAC_DMA_ADDRESS_BITS, 40);
        assert_eq!(DmaInterruptEnable::NORMAL.bits(), 1 << 15);
        assert_eq!(DmaInterruptEnable::ABNORMAL.bits(), 1 << 14);
        assert_eq!(DWMAC_DMA_STATUS_W1C.bits(), 0x003f_ffc7);
        let feature2 = GmacHwFeature2::from_bits_retain(0);
        assert_eq!(
            ((feature2.bits() & GmacHwFeature2::RX_QUEUE_MASK.bits()) + 1) as u8,
            1
        );
        assert_eq!(
            (((feature2.bits() & GmacHwFeature2::TX_QUEUE_MASK.bits()) >> 6) + 1) as u8,
            1
        );
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
