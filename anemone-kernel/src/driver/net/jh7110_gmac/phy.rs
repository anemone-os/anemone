use super::{fwnode::GmacPhyConfig, regs::GmacRegs};
use crate::prelude::*;

mod register {
    pub const BMCR: u8 = 0;
    pub const BMSR: u8 = 1;
    pub const PHY_ID1: u8 = 2;
    pub const PHY_ID2: u8 = 3;
    pub const SPECIFIC_STATUS: u8 = 0x11;
    pub const EXTENDED_ADDRESS: u8 = 0x1e;
    pub const EXTENDED_DATA: u8 = 0x1f;
}

mod extended_register {
    pub const CHIP_CONFIG: u16 = 0xa001;
    pub const RGMII_CONFIG1: u16 = 0xa003;
    pub const PAD_DRIVE_STRENGTH: u16 = 0xa010;
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct BasicControl: u16 {
        const RESET = 1 << 15;
        const AUTO_NEGOTIATION_ENABLE = 1 << 12;
        const POWER_DOWN = 1 << 11;
        const ISOLATE = 1 << 10;
        const RESTART_AUTO_NEGOTIATION = 1 << 9;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct SpecificStatus: u16 {
        const SPEED_MASK = 0b11 << 14;
        const FULL_DUPLEX = 1 << 13;
        const SPEED_DUPLEX_RESOLVED = 1 << 11;
        const LINK = 1 << 10;
        const _ = !0;
    }
}

const PHY_ID_YT8521: u32 = 0x0000_011a;
const PHY_ID_YT8531: u32 = 0x4f51_e91b;
const PHY_ID_YT8531S: u32 = 0x4f51_e91a;

static_assert!(
    JH7110_GMAC_PHY_TIMEOUT_MS > 0,
    "JH7110_GMAC_PHY_TIMEOUT_MS must be non-zero"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PhyLink {
    pub(super) speed_mbps: u32,
    pub(super) full_duplex: bool,
}

pub(super) fn initialize(
    regs: &GmacRegs,
    config: GmacPhyConfig,
) -> Result<Option<PhyLink>, SysError> {
    let id1 = regs.mdio_read(config.address, register::PHY_ID1)?;
    let id2 = regs.mdio_read(config.address, register::PHY_ID2)?;
    let phy_id = ((id1 as u32) << 16) | id2 as u32;
    if !matches!(phy_id, PHY_ID_YT8521 | PHY_ID_YT8531 | PHY_ID_YT8531S) {
        kerrln!(
            "jh7110-gmac: unsupported PHY address={} id={:#010x}",
            config.address,
            phy_id,
        );
        return Err(SysError::DriverIncompatible);
    }

    soft_reset(regs, config.address)?;
    configure_motorcomm(regs, config)?;
    restart_auto_negotiation(regs, config.address)?;
    let link = wait_for_link(regs, config.address)?;
    let selected_speed = link.map_or(1000, |link| link.speed_mbps);
    configure_tx_clock_inversion(regs, config, selected_speed)?;

    let bmcr = regs.mdio_read(config.address, register::BMCR)?;
    let bmsr = regs.mdio_read(config.address, register::BMSR)?;
    let status = regs.mdio_read(config.address, register::SPECIFIC_STATUS)?;
    kinfoln!(
        "jh7110-gmac: PHY initialized address={} id={:#010x} bmcr={:#06x} bmsr={:#06x} status={:#06x} link={:?}",
        config.address,
        phy_id,
        bmcr,
        bmsr,
        status,
        link,
    );
    Ok(link)
}

fn soft_reset(regs: &GmacRegs, address: u8) -> Result<(), SysError> {
    let control = BasicControl::from_bits_retain(regs.mdio_read(address, register::BMCR)?);
    regs.mdio_write(
        address,
        register::BMCR,
        (control | BasicControl::RESET).bits(),
    )?;
    let start = Instant::now();
    let timeout = Duration::from_millis(JH7110_GMAC_RESET_TIMEOUT_MS);
    loop {
        let control = BasicControl::from_bits_retain(regs.mdio_read(address, register::BMCR)?);
        if !control.contains(BasicControl::RESET) {
            return Ok(());
        }
        if start.elapsed() >= timeout {
            kerrln!(
                "jh7110-gmac: PHY reset timeout address={} bmcr={:#06x}",
                address,
                control.bits(),
            );
            return Err(SysError::Timeout);
        }
        core::hint::spin_loop();
    }
}

fn configure_motorcomm(regs: &GmacRegs, config: GmacPhyConfig) -> Result<(), SysError> {
    if let Some(enabled) = config.rxc_delay_enable {
        modify_extended(
            regs,
            config.address,
            extended_register::CHIP_CONFIG,
            1 << 8,
            (enabled as u16) << 8,
        )?;
    }
    modify_extended(
        regs,
        config.address,
        extended_register::PAD_DRIVE_STRENGTH,
        (0b111 << 13) | (1 << 12) | (0b11 << 4),
        ((config.rgmii_rxc_drive as u16) << 13)
            | ((config.rgmii_drive_high as u16) << 12)
            | ((config.rgmii_drive as u16) << 4),
    )?;
    modify_extended(
        regs,
        config.address,
        extended_register::RGMII_CONFIG1,
        (0xf << 10) | (0xf << 4) | 0xf,
        ((config.rx_delay as u16) << 10)
            | ((config.tx_delay_fe as u16) << 4)
            | config.tx_delay as u16,
    )
}

fn restart_auto_negotiation(regs: &GmacRegs, address: u8) -> Result<(), SysError> {
    let mut control = BasicControl::from_bits_retain(regs.mdio_read(address, register::BMCR)?);
    control.remove(BasicControl::POWER_DOWN | BasicControl::ISOLATE);
    control.insert(BasicControl::AUTO_NEGOTIATION_ENABLE | BasicControl::RESTART_AUTO_NEGOTIATION);
    regs.mdio_write(address, register::BMCR, control.bits())
}

fn wait_for_link(regs: &GmacRegs, address: u8) -> Result<Option<PhyLink>, SysError> {
    let start = Instant::now();
    let timeout = Duration::from_millis(JH7110_GMAC_PHY_TIMEOUT_MS);
    loop {
        let status =
            SpecificStatus::from_bits_retain(regs.mdio_read(address, register::SPECIFIC_STATUS)?);
        if let Some(link) = decode_link(status)? {
            return Ok(Some(link));
        }
        if start.elapsed() >= timeout {
            kwarningln!(
                "jh7110-gmac: PHY link unresolved after {} ms address={} status={:#06x}; publishing with 1000/full MAC default",
                JH7110_GMAC_PHY_TIMEOUT_MS,
                address,
                status.bits(),
            );
            return Ok(None);
        }
        core::hint::spin_loop();
    }
}

fn decode_link(status: SpecificStatus) -> Result<Option<PhyLink>, SysError> {
    if !status.contains(SpecificStatus::LINK | SpecificStatus::SPEED_DUPLEX_RESOLVED) {
        return Ok(None);
    }
    let speed_mbps = match (status.bits() & SpecificStatus::SPEED_MASK.bits()) >> 14 {
        0 => 10,
        1 => 100,
        2 => 1000,
        _ => return Err(SysError::DriverIncompatible),
    };
    Ok(Some(PhyLink {
        speed_mbps,
        full_duplex: status.contains(SpecificStatus::FULL_DUPLEX),
    }))
}

fn configure_tx_clock_inversion(
    regs: &GmacRegs,
    config: GmacPhyConfig,
    speed_mbps: u32,
) -> Result<(), SysError> {
    let inverted = match speed_mbps {
        10 => config.tx_inverted_10,
        100 => config.tx_inverted_100,
        1000 => config.tx_inverted_1000,
        _ => return Err(SysError::InvalidArgument),
    };
    modify_extended(
        regs,
        config.address,
        extended_register::RGMII_CONFIG1,
        1 << 14,
        (inverted as u16) << 14,
    )
}

fn modify_extended(
    regs: &GmacRegs,
    address: u8,
    register: u16,
    mask: u16,
    value: u16,
) -> Result<(), SysError> {
    regs.mdio_write(address, register::EXTENDED_ADDRESS, register)?;
    let current = regs.mdio_read(address, register::EXTENDED_DATA)?;
    regs.mdio_write(
        address,
        register::EXTENDED_DATA,
        (current & !mask) | (value & mask),
    )
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn motorcomm_status_requires_resolved_link_and_decodes_speed_duplex() {
        assert_eq!(decode_link(SpecificStatus::empty()), Ok(None));
        assert_eq!(
            decode_link(SpecificStatus::LINK | SpecificStatus::SPEED_DUPLEX_RESOLVED),
            Ok(Some(PhyLink {
                speed_mbps: 10,
                full_duplex: false,
            }))
        );
        assert_eq!(
            decode_link(
                SpecificStatus::LINK
                    | SpecificStatus::SPEED_DUPLEX_RESOLVED
                    | SpecificStatus::FULL_DUPLEX
                    | SpecificStatus::from_bits_retain(2 << 14),
            ),
            Ok(Some(PhyLink {
                speed_mbps: 1000,
                full_duplex: true,
            }))
        );
        assert_eq!(
            decode_link(
                SpecificStatus::LINK
                    | SpecificStatus::SPEED_DUPLEX_RESOLVED
                    | SpecificStatus::SPEED_MASK,
            ),
            Err(SysError::DriverIncompatible)
        );
    }
}
