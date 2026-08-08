use crate::{mm::remap::IoRemap, prelude::*};

// Fixed DWMAC 5.20 register facts are cross-checked against
// xref:linux-6.6.32:drivers/net/ethernet/stmicro/stmmac/{dwmac4.h,dwmac4_dma.h,
// hwif.h}.

mod offsets {
    pub const GMAC_INT_ENABLE: usize = 0x00b4;
    pub const GMAC_HW_FEATURE0: usize = 0x011c;
    pub const GMAC_HW_FEATURE1: usize = 0x0120;
    pub const GMAC_HW_FEATURE2: usize = 0x0124;
    pub const GMAC_HW_FEATURE3: usize = 0x0128;
    pub const GMAC4_VERSION: usize = 0x0110;
    pub const DMA_CHANNEL0_BASE: usize = 0x1100;
    pub const DMA_CHANNEL_INTERRUPT_ENABLE: usize = 0x34;
    pub const DMA_CHANNEL_STATUS: usize = 0x60;
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GmacCapabilities {
    pub(super) version: u8,
    pub(super) hw_feature0: u32,
    pub(super) hw_feature1: u32,
    pub(super) hw_feature2: u32,
    pub(super) hw_feature3: u32,
    pub(super) rx_queues: u8,
    pub(super) tx_queues: u8,
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
        unsafe { core::ptr::read_volatile(self.ptr_at(offset)) }
    }

    fn write(&self, offset: usize, value: u32) {
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
        })
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
        let status = self.read(offsets::DMA_CHANNEL0_STATUS);
        self.write(offsets::DMA_CHANNEL0_STATUS, status);
    }

    #[allow(dead_code)]
    // Gate 1 owns the first production consumer and removes this allowance
    // when the handler/ring protocol is ready for device-side enable.
    pub(super) fn enable_device_interrupts(&self, mac_mask: u32, dma_mask: u32) {
        self.write(offsets::GMAC_INT_ENABLE, mac_mask);
        self.write(offsets::DMA_CHANNEL0_INTERRUPT_ENABLE, dma_mask);
    }
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
}
