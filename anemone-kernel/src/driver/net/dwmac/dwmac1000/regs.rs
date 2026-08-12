use crate::{mm::remap::IoRemap, prelude::*, time::MonotonicInstant};

const DMA_BUS_MODE: usize = 0x1000;
const MAC_MII_ADDR: usize = 0x0010;
const MAC_MII_DATA: usize = 0x0014;
const MAC_VERSION: usize = 0x0020;
const DMA_HW_FEATURE: usize = 0x1058;
const EXPECTED_CORE_VERSION: u8 = 0x37;
const SW_RESET: u32 = 1;
const ALTERNATE_DESCRIPTOR_SIZE: u32 = 1 << 7;
const MII_BUSY: u32 = 1;
const MII_CLOCK_RANGE: u32 = 2 << 2;
const ENHDESSEL: u32 = 1 << 24;
const PHY_ID_YT8511: u32 = 0x0000_010a;

#[derive(Debug, Clone, Copy)]
pub(super) struct Dwmac1000Capabilities {
    pub(super) version: u32,
    pub(super) hw_feature: u32,
    pub(super) enhanced_descriptors: bool,
}

impl Dwmac1000Capabilities {
    pub(super) const fn expected_family(self) -> bool {
        self.version as u8 == EXPECTED_CORE_VERSION
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

pub(super) struct Dwmac1000Regs {
    remap: IoRemap,
}

impl Dwmac1000Regs {
    const REQUIRED_MAPPING_LEN: usize = DMA_HW_FEATURE + 4;

    pub(super) fn new(remap: IoRemap) -> Result<Self, SysError> {
        if remap.size() < Self::REQUIRED_MAPPING_LEN as u64 {
            return Err(SysError::DriverIncompatible);
        }
        Ok(Self { remap })
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

    pub(super) const fn mdio_divider(&self) -> u32 {
        MII_CLOCK_RANGE >> 2
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
        self.write(MAC_MII_ADDR, mdio_read_command(phy, register));
        self.wait_mdio_idle()?;
        Ok(self.read(MAC_MII_DATA) as u16)
    }

    pub(super) fn mdio_write(&self, phy: u8, register: u8, value: u16) -> Result<(), SysError> {
        self.wait_mdio_idle()?;
        self.write(MAC_MII_DATA, value as u32);
        self.write(
            MAC_MII_ADDR,
            MII_BUSY | (1 << 1) | (register as u32) << 6 | (phy as u32) << 11 | MII_CLOCK_RANGE,
        );
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
}

const fn decode_capabilities(version: u32, hw_feature: u32) -> Dwmac1000Capabilities {
    Dwmac1000Capabilities {
        version,
        hw_feature,
        enhanced_descriptors: hw_feature & ENHDESSEL != 0,
    }
}

const fn mdio_read_command(phy: u8, register: u8) -> u32 {
    MII_BUSY | (register as u32) << 6 | (phy as u32) << 11 | MII_CLOCK_RANGE
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn capability_log_reports_enhanced_support_without_selecting_it() {
        let normal = decode_capabilities(0x37, 0);
        assert!(!normal.enhanced_descriptors);
        let enhanced = decode_capabilities(0x37, ENHDESSEL);
        assert!(enhanced.enhanced_descriptors);
    }

    #[kunit]
    fn mdio_read_command_keeps_divider_phy_and_register_fields_separate() {
        let command = mdio_read_command(3, 2);
        assert_eq!(command & 1, MII_BUSY);
        assert_eq!((command >> 2) & 0xf, MII_CLOCK_RANGE >> 2);
        assert_eq!((command >> 6) & 0x1f, 2);
        assert_eq!((command >> 11) & 0x1f, 3);
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
}
