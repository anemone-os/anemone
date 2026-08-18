//! Motorcomm YT8511 config_init and generic Clause 22 link resolution.
//!
//! Linux 6.6's YT8511 entry only supplies `config_init`; it inherits generic
//! phylib autonegotiation and status handling. Keep that split explicit so a
//! vendor status register or board observation cannot become protocol truth.

use crate::{prelude::*, time::MonotonicInstant};

use super::regs::Dwmac1000Regs;

const PHY_ID_YT8511: u32 = 0x0000_010a;

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub(super) enum Clause22Register {
    BasicControl = 0,
    BasicStatus = 1,
    PhyId1 = 2,
    PhyId2 = 3,
    Advertisement = 4,
    LinkPartnerAbility = 5,
    GigabitControl = 9,
    GigabitStatus = 10,
    ExtendedStatus = 15,
    PageSelect = 0x1e,
    PageData = 0x1f,
}

#[derive(Debug, Clone, Copy)]
#[repr(u16)]
enum Yt8511Page {
    ClockGate = 0x000c,
    DelayDrive = 0x000d,
    SleepControl = 0x0027,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct BasicControl: u16 {
        const AUTO_NEGOTIATION_ENABLE = 1 << 12;
        const ISOLATE = 1 << 10;
        const RESTART_AUTO_NEGOTIATION = 1 << 9;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct BasicStatus: u16 {
        const LINK = 1 << 2;
        const AUTO_NEGOTIATION_CAPABLE = 1 << 3;
        const AUTO_NEGOTIATION_COMPLETE = 1 << 5;
        const EXTENDED_STATUS = 1 << 8;
        const MODE_10_HALF = 1 << 11;
        const MODE_10_FULL = 1 << 12;
        const MODE_100_HALF = 1 << 13;
        const MODE_100_FULL = 1 << 14;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ExtendedStatus: u16 {
        const MODE_1000_T_HALF = 1 << 12;
        const MODE_1000_T_FULL = 1 << 13;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Advertisement: u16 {
        const SELECTOR_MASK = 0x1f;
        const CSMA = 1;
        const MODE_10_HALF = 1 << 5;
        const MODE_10_FULL = 1 << 6;
        const MODE_100_HALF = 1 << 7;
        const MODE_100_FULL = 1 << 8;
        const PAUSE = 1 << 10;
        const ASYMMETRIC_PAUSE = 1 << 11;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct GigabitControl: u16 {
        const ADVERTISE_HALF = 1 << 8;
        const ADVERTISE_FULL = 1 << 9;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct GigabitStatus: u16 {
        const LINK_PARTNER_HALF = 1 << 10;
        const LINK_PARTNER_FULL = 1 << 11;
        const MASTER_SLAVE_FAILURE = 1 << 15;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ClockGate: u16 {
        const RX_DELAY = 1;
        const CLOCK_125MHZ = 0x0006;
        const GE_TX_DELAY_MASK = 0x00f0;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct DelayDrive: u16 {
        const FE_TX_DELAY_MASK = 0xf000;
        const _ = !0;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct SleepControl: u16 {
        const PLL_ON_IN_SLEEP = 1 << 14;
        const _ = !0;
    }
}

const DELAY_GE_TX_DIS: u16 = 0x0020;
const DELAY_FE_TX_DIS: u16 = 0x2000;

static_assert!(
    DWMAC1000_PHY_TIMEOUT_MS > 0,
    "DWMAC1000_PHY_TIMEOUT_MS must be greater than zero"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PhyLink {
    pub(super) speed_mbps: u32,
    pub(super) full_duplex: bool,
    pub(super) rx_pause: bool,
    pub(super) tx_pause: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PhyState {
    pub(super) bmcr: u16,
    pub(super) bmsr: u16,
    pub(super) advertise: u16,
    pub(super) lpa: u16,
    pub(super) ctrl1000: u16,
    pub(super) stat1000: u16,
    pub(super) autoneg_restarted: bool,
    pub(super) link: PhyLink,
    pub(super) delay_before: DelayRegisters,
    pub(super) delay_configured: DelayRegisters,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DelayRegisters {
    pub(super) clk_gate: u16,
    pub(super) delay_drive: u16,
    pub(super) sleep_ctrl: u16,
}

pub(super) fn initialize_yt8511(
    regs: &Dwmac1000Regs,
    address: u8,
    mode: &str,
    max_speed: Option<u32>,
    mii: bool,
    gmii: bool,
    half_duplex: bool,
) -> Result<Option<PhyState>, SysError> {
    let id1 = regs.mdio_read(address, Clause22Register::PhyId1)?;
    let id2 = regs.mdio_read(address, Clause22Register::PhyId2)?;
    if ((id1 as u32) << 16) | id2 as u32 != PHY_ID_YT8511 {
        return Err(SysError::DriverIncompatible);
    }

    // YT8511 has no driver-specific soft_reset in Linux 6.6. Its hardware
    // initialization is exactly this config_init page transaction.
    let (delay_before, delay_configured) = configure_delay(regs, address, mode)?;
    let autoneg_restarted =
        configure_generic_autoneg(regs, address, max_speed, mii, gmii, half_duplex)?;
    let Some(link) = wait_generic_link(regs, address)? else {
        return Ok(None);
    };
    Ok(Some(PhyState {
        bmcr: link.bmcr,
        bmsr: link.bmsr,
        advertise: link.advertise,
        lpa: link.lpa,
        ctrl1000: link.ctrl1000,
        stat1000: link.stat1000,
        autoneg_restarted,
        link: link.link,
        delay_before,
        delay_configured,
    }))
}

fn configure_generic_autoneg(
    regs: &Dwmac1000Regs,
    address: u8,
    max_speed: Option<u32>,
    mii: bool,
    gmii: bool,
    half_duplex: bool,
) -> Result<bool, SysError> {
    let bmsr = regs.mdio_read(address, Clause22Register::BasicStatus)?;
    if bmsr & BasicStatus::AUTO_NEGOTIATION_CAPABLE.bits() == 0 {
        return Err(SysError::DriverIncompatible);
    }
    let supported_10_100 = filter_advertised_10_100(
        advertised_10_100_from_bmsr(bmsr),
        max_speed,
        mii,
        half_duplex,
    );
    let ctrl1000 = if bmsr & BasicStatus::EXTENDED_STATUS.bits() != 0 {
        let estatus = regs.mdio_read(address, Clause22Register::ExtendedStatus)?;
        let before = regs.mdio_read(address, Clause22Register::GigabitControl)?;
        let advertised = filter_advertised_1000(estatus, max_speed, gmii, half_duplex);
        Some((
            before,
            (before
                & !(GigabitControl::ADVERTISE_HALF.bits() | GigabitControl::ADVERTISE_FULL.bits()))
                | advertised,
        ))
    } else {
        None
    };
    if supported_10_100 == 0
        && ctrl1000.is_none_or(|(_, value)| {
            value & (GigabitControl::ADVERTISE_HALF.bits() | GigabitControl::ADVERTISE_FULL.bits())
                == 0
        })
    {
        kerrln!(
            "dwmac1000 stage=phy-advertisement result=fail reason=no-mac-compatible-mode phy-address={} mii={} gmii={} half-duplex={} max-speed={:?}",
            address,
            mii,
            gmii,
            half_duplex,
            max_speed,
        );
        return Err(SysError::DriverIncompatible);
    }

    let advertise_before = regs.mdio_read(address, Clause22Register::Advertisement)?;
    let advertise = (advertise_before
        & !(Advertisement::SELECTOR_MASK.bits()
            | Advertisement::MODE_10_HALF.bits()
            | Advertisement::MODE_10_FULL.bits()
            | Advertisement::MODE_100_HALF.bits()
            | Advertisement::MODE_100_FULL.bits()))
        | Advertisement::CSMA.bits()
        | supported_10_100
        // phylib supplies both pause capabilities for a normal external PHY
        // before phylink validates them against this MAC's symmetric/asymmetric
        // pause support.
        | Advertisement::PAUSE.bits()
        | Advertisement::ASYMMETRIC_PAUSE.bits();
    let mut changed = advertise != advertise_before;
    if changed {
        regs.mdio_write(address, Clause22Register::Advertisement, advertise)?;
    }

    if let Some((before, configured)) = ctrl1000 {
        if configured != before {
            regs.mdio_write(address, Clause22Register::GigabitControl, configured)?;
            changed = true;
        }
    }

    // This is Linux genphy_restart_aneg: clear isolate, enable and restart
    // Clause 22 autonegotiation after advertising supported link modes.
    let bmcr = regs.mdio_read(address, Clause22Register::BasicControl)?;
    let restart = changed
        || bmcr & BasicControl::AUTO_NEGOTIATION_ENABLE.bits() == 0
        || bmcr & BasicControl::ISOLATE.bits() != 0;
    if restart {
        regs.mdio_write(
            address,
            Clause22Register::BasicControl,
            (bmcr & !BasicControl::ISOLATE.bits())
                | BasicControl::AUTO_NEGOTIATION_ENABLE.bits()
                | BasicControl::RESTART_AUTO_NEGOTIATION.bits(),
        )?;
    }
    Ok(restart)
}

const fn filter_advertised_10_100(
    modes: u16,
    max_speed: Option<u32>,
    mii: bool,
    half_duplex: bool,
) -> u16 {
    if !mii {
        return 0;
    }
    let mut modes = modes;
    if !half_duplex {
        modes &= !(Advertisement::MODE_10_HALF.bits() | Advertisement::MODE_100_HALF.bits());
    }
    match max_speed {
        Some(10) => {
            modes & (Advertisement::MODE_10_HALF.bits() | Advertisement::MODE_10_FULL.bits())
        },
        Some(100) => modes,
        _ => modes,
    }
}

const fn filter_advertised_1000(
    estatus: u16,
    max_speed: Option<u32>,
    gmii: bool,
    half_duplex: bool,
) -> u16 {
    if !gmii || matches!(max_speed, Some(10 | 100)) {
        return 0;
    }
    (if half_duplex && estatus & ExtendedStatus::MODE_1000_T_HALF.bits() != 0 {
        GigabitControl::ADVERTISE_HALF.bits()
    } else {
        0
    }) | (if estatus & ExtendedStatus::MODE_1000_T_FULL.bits() != 0 {
        GigabitControl::ADVERTISE_FULL.bits()
    } else {
        0
    })
}

#[derive(Debug, Clone, Copy)]
struct GenericLinkSnapshot {
    bmcr: u16,
    bmsr: u16,
    advertise: u16,
    lpa: u16,
    ctrl1000: u16,
    stat1000: u16,
    link: PhyLink,
}

fn wait_generic_link(
    regs: &Dwmac1000Regs,
    address: u8,
) -> Result<Option<GenericLinkSnapshot>, SysError> {
    let start = MonotonicInstant::now();
    loop {
        let bmcr = regs.mdio_read(address, Clause22Register::BasicControl)?;
        // BMSR link is latched low; generic phylib consumes two samples while
        // polling from an initially down state.
        let _ = regs.mdio_read(address, Clause22Register::BasicStatus)?;
        let bmsr = regs.mdio_read(address, Clause22Register::BasicStatus)?;
        if bmcr & BasicControl::RESTART_AUTO_NEGOTIATION.bits() == 0
            && bmsr & (BasicStatus::LINK.bits() | BasicStatus::AUTO_NEGOTIATION_COMPLETE.bits())
                == BasicStatus::LINK.bits() | BasicStatus::AUTO_NEGOTIATION_COMPLETE.bits()
        {
            let advertise = regs.mdio_read(address, Clause22Register::Advertisement)?;
            let lpa = regs.mdio_read(address, Clause22Register::LinkPartnerAbility)?;
            let ctrl1000 = regs.mdio_read(address, Clause22Register::GigabitControl)?;
            let stat1000 = regs.mdio_read(address, Clause22Register::GigabitStatus)?;
            let link = resolve_autoneg(advertise, lpa, ctrl1000, stat1000)?;
            return Ok(Some(GenericLinkSnapshot {
                bmcr,
                bmsr,
                advertise,
                lpa,
                ctrl1000,
                stat1000,
                link,
            }));
        }
        if start.elapsed() >= Duration::from_millis(DWMAC1000_PHY_TIMEOUT_MS) {
            kwarningln!(
                "dwmac1000 stage=phy-link result=unresolved reason=timeout action=publish-placeholder publication-link=down restart-with-carrier-required=true protocol=generic-clause22 deadline-ms={} phy-address={} bmcr={:#06x} bmsr={:#06x}",
                DWMAC1000_PHY_TIMEOUT_MS,
                address,
                bmcr,
                bmsr,
            );
            return Ok(None);
        }
        core::hint::spin_loop();
    }
}

const fn advertised_10_100_from_bmsr(bmsr: u16) -> u16 {
    (if bmsr & BasicStatus::MODE_10_HALF.bits() != 0 {
        Advertisement::MODE_10_HALF.bits()
    } else {
        0
    }) | (if bmsr & BasicStatus::MODE_10_FULL.bits() != 0 {
        Advertisement::MODE_10_FULL.bits()
    } else {
        0
    }) | (if bmsr & BasicStatus::MODE_100_HALF.bits() != 0 {
        Advertisement::MODE_100_HALF.bits()
    } else {
        0
    }) | (if bmsr & BasicStatus::MODE_100_FULL.bits() != 0 {
        Advertisement::MODE_100_FULL.bits()
    } else {
        0
    })
}

const fn resolve_autoneg(
    advertise: u16,
    lpa: u16,
    ctrl1000: u16,
    stat1000: u16,
) -> Result<PhyLink, SysError> {
    if stat1000 & GigabitStatus::MASTER_SLAVE_FAILURE.bits() != 0 {
        return Err(SysError::DriverIncompatible);
    }
    let (rx_pause, tx_pause) = resolve_pause(advertise, lpa);
    let gigabit = ctrl1000
        & ((stat1000 >> 2)
            & (GigabitControl::ADVERTISE_HALF.bits() | GigabitControl::ADVERTISE_FULL.bits()));
    if gigabit & GigabitControl::ADVERTISE_FULL.bits() != 0 {
        return Ok(PhyLink {
            speed_mbps: 1000,
            full_duplex: true,
            rx_pause,
            tx_pause,
        });
    }
    if gigabit & GigabitControl::ADVERTISE_HALF.bits() != 0 {
        return Ok(PhyLink {
            speed_mbps: 1000,
            full_duplex: false,
            rx_pause: false,
            tx_pause: false,
        });
    }
    let common = advertise & lpa;
    if common & Advertisement::MODE_100_FULL.bits() != 0 {
        return Ok(PhyLink {
            speed_mbps: 100,
            full_duplex: true,
            rx_pause,
            tx_pause,
        });
    }
    if common & Advertisement::MODE_100_HALF.bits() != 0 {
        return Ok(PhyLink {
            speed_mbps: 100,
            full_duplex: false,
            rx_pause: false,
            tx_pause: false,
        });
    }
    if common & Advertisement::MODE_10_FULL.bits() != 0 {
        return Ok(PhyLink {
            speed_mbps: 10,
            full_duplex: true,
            rx_pause,
            tx_pause,
        });
    }
    if common & Advertisement::MODE_10_HALF.bits() != 0 {
        return Ok(PhyLink {
            speed_mbps: 10,
            full_duplex: false,
            rx_pause: false,
            tx_pause: false,
        });
    }
    Err(SysError::DriverIncompatible)
}

const fn resolve_pause(advertise: u16, lpa: u16) -> (bool, bool) {
    if advertise & lpa & Advertisement::PAUSE.bits() != 0 {
        (true, true)
    } else if advertise & lpa & Advertisement::ASYMMETRIC_PAUSE.bits() != 0 {
        if advertise & Advertisement::PAUSE.bits() != 0 {
            (true, false)
        } else if lpa & Advertisement::PAUSE.bits() != 0 {
            (false, true)
        } else {
            (false, false)
        }
    } else {
        (false, false)
    }
}

fn configure_delay(
    regs: &Dwmac1000Regs,
    address: u8,
    mode: &str,
) -> Result<(DelayRegisters, DelayRegisters), SysError> {
    let (ge, fe) = delay_bits(mode)?;
    let old_page = regs.mdio_read(address, Clause22Register::PageSelect)?;
    let result = configure_selected_pages(regs, address, ge, fe);
    let restore = regs.mdio_write(address, Clause22Register::PageSelect, old_page);
    finish_page_transaction(address, old_page, result, restore)
}

fn configure_selected_pages(
    regs: &Dwmac1000Regs,
    address: u8,
    ge: u16,
    fe: u16,
) -> Result<(DelayRegisters, DelayRegisters), SysError> {
    regs.mdio_write(
        address,
        Clause22Register::PageSelect,
        Yt8511Page::ClockGate as u16,
    )?;
    let clk_gate = regs.mdio_read(address, Clause22Register::PageData)?;
    let delay_value = modify_bits(
        clk_gate,
        ClockGate::RX_DELAY.bits() | ClockGate::GE_TX_DELAY_MASK.bits(),
        ge,
    );
    regs.mdio_write(address, Clause22Register::PageData, delay_value)?;
    regs.mdio_write(
        address,
        Clause22Register::PageData,
        delay_value | ClockGate::CLOCK_125MHZ.bits(),
    )?;

    regs.mdio_write(
        address,
        Clause22Register::PageSelect,
        Yt8511Page::DelayDrive as u16,
    )?;
    let delay_drive = regs.mdio_read(address, Clause22Register::PageData)?;
    regs.mdio_write(
        address,
        Clause22Register::PageData,
        modify_bits(delay_drive, DelayDrive::FE_TX_DELAY_MASK.bits(), fe),
    )?;

    regs.mdio_write(
        address,
        Clause22Register::PageSelect,
        Yt8511Page::SleepControl as u16,
    )?;
    let sleep_ctrl = regs.mdio_read(address, Clause22Register::PageData)?;
    regs.mdio_write(
        address,
        Clause22Register::PageData,
        sleep_ctrl | SleepControl::PLL_ON_IN_SLEEP.bits(),
    )?;

    let before = DelayRegisters {
        clk_gate,
        delay_drive,
        sleep_ctrl,
    };
    let after = read_selected_delay_registers(regs, address)?;
    if !delay_registers_match(after, ge, fe) {
        kerrln!(
            "dwmac1000 stage=phy-config-init result=fail reason=readback-mismatch phy-address={} page0c-before={:#06x} page0c-after={:#06x} page0d-before={:#06x} page0d-after={:#06x} page27-before={:#06x} page27-after={:#06x}",
            address,
            before.clk_gate,
            after.clk_gate,
            before.delay_drive,
            after.delay_drive,
            before.sleep_ctrl,
            after.sleep_ctrl,
        );
        return Err(SysError::DriverIncompatible);
    }
    Ok((before, after))
}

fn finish_page_transaction<T>(
    address: u8,
    old_page: u16,
    operation: Result<T, SysError>,
    restore: Result<(), SysError>,
) -> Result<T, SysError> {
    let error = page_transaction_error(
        operation.as_ref().err().copied(),
        restore.as_ref().err().copied(),
    );
    match (operation, restore) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(operation_error), Ok(())) => {
            kerrln!(
                "dwmac1000 stage=phy-page result=fail phy-address={} operation-error={:?} cleanup=restored old-page={:#06x}",
                address,
                operation_error,
                old_page,
            );
            Err(error.unwrap())
        },
        (Ok(_), Err(restore_error)) => {
            kerrln!(
                "dwmac1000 stage=phy-page result=fail phy-address={} cleanup=restore-failed old-page={:#06x} error={:?}",
                address,
                old_page,
                restore_error,
            );
            Err(error.unwrap())
        },
        (Err(operation_error), Err(restore_error)) => {
            // Match Linux phy_restore_page(): the earliest operation error
            // remains authoritative even when restoring the page also fails.
            kerrln!(
                "dwmac1000 stage=phy-page result=fail phy-address={} operation-error={:?} cleanup=restore-failed old-page={:#06x} restore-error={:?}",
                address,
                operation_error,
                old_page,
                restore_error,
            );
            Err(error.unwrap())
        },
    }
}

const fn page_transaction_error(
    operation: Option<SysError>,
    restore: Option<SysError>,
) -> Option<SysError> {
    match operation {
        Some(error) => Some(error),
        None => restore,
    }
}

fn read_selected_delay_registers(
    regs: &Dwmac1000Regs,
    address: u8,
) -> Result<DelayRegisters, SysError> {
    regs.mdio_write(
        address,
        Clause22Register::PageSelect,
        Yt8511Page::ClockGate as u16,
    )?;
    let clk_gate = regs.mdio_read(address, Clause22Register::PageData)?;
    regs.mdio_write(
        address,
        Clause22Register::PageSelect,
        Yt8511Page::DelayDrive as u16,
    )?;
    let delay_drive = regs.mdio_read(address, Clause22Register::PageData)?;
    regs.mdio_write(
        address,
        Clause22Register::PageSelect,
        Yt8511Page::SleepControl as u16,
    )?;
    let sleep_ctrl = regs.mdio_read(address, Clause22Register::PageData)?;
    Ok(DelayRegisters {
        clk_gate,
        delay_drive,
        sleep_ctrl,
    })
}

const fn modify_bits(current: u16, clear: u16, set: u16) -> u16 {
    (current & !clear) | set
}

const fn delay_bits(mode: &str) -> Result<(u16, u16), SysError> {
    match mode.as_bytes() {
        b"rgmii" => Ok((DELAY_GE_TX_DIS, DELAY_FE_TX_DIS)),
        b"rgmii-id" => Ok((
            ClockGate::RX_DELAY.bits() | ClockGate::GE_TX_DELAY_MASK.bits(),
            DelayDrive::FE_TX_DELAY_MASK.bits(),
        )),
        _ => Err(SysError::DriverIncompatible),
    }
}

const fn delay_registers_match(registers: DelayRegisters, ge: u16, fe: u16) -> bool {
    registers.clk_gate
        & (ClockGate::RX_DELAY.bits()
            | ClockGate::GE_TX_DELAY_MASK.bits()
            | ClockGate::CLOCK_125MHZ.bits())
        == ge | ClockGate::CLOCK_125MHZ.bits()
        && registers.delay_drive & DelayDrive::FE_TX_DELAY_MASK.bits() == fe
        && registers.sleep_ctrl & SleepControl::PLL_ON_IN_SLEEP.bits() != 0
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn rgmii_delay_modes_match_linux_motorcomm_config_init() {
        assert_eq!(delay_bits("rgmii"), Ok((DELAY_GE_TX_DIS, DELAY_FE_TX_DIS)));
        assert_eq!(
            delay_bits("rgmii-id"),
            Ok((
                ClockGate::RX_DELAY.bits() | ClockGate::GE_TX_DELAY_MASK.bits(),
                DelayDrive::FE_TX_DELAY_MASK.bits(),
            ))
        );
        assert_eq!(delay_bits("sgmii"), Err(SysError::DriverIncompatible));
    }

    #[kunit]
    fn generic_resolution_prefers_fastest_common_clause22_mode() {
        assert_eq!(
            resolve_autoneg(
                GigabitControl::ADVERTISE_FULL.bits() | Advertisement::MODE_100_FULL.bits(),
                Advertisement::MODE_100_FULL.bits(),
                GigabitControl::ADVERTISE_FULL.bits(),
                GigabitStatus::LINK_PARTNER_FULL.bits(),
            ),
            Ok(PhyLink {
                speed_mbps: 1000,
                full_duplex: true,
                rx_pause: false,
                tx_pause: false,
            })
        );
        assert_eq!(
            resolve_autoneg(
                Advertisement::MODE_100_FULL.bits() | Advertisement::MODE_10_FULL.bits(),
                Advertisement::MODE_100_FULL.bits() | Advertisement::MODE_10_FULL.bits(),
                0,
                0,
            ),
            Ok(PhyLink {
                speed_mbps: 100,
                full_duplex: true,
                rx_pause: false,
                tx_pause: false,
            })
        );
    }

    #[kunit]
    fn generic_resolution_fails_without_common_mode_or_on_master_slave_failure() {
        assert_eq!(
            resolve_autoneg(0, 0, 0, 0),
            Err(SysError::DriverIncompatible)
        );
        assert_eq!(
            resolve_autoneg(0, 0, 0, GigabitStatus::MASTER_SLAVE_FAILURE.bits()),
            Err(SysError::DriverIncompatible)
        );
    }

    #[kunit]
    fn max_speed_and_mac_capability_filter_phy_advertisement() {
        let all = Advertisement::MODE_10_HALF.bits()
            | Advertisement::MODE_10_FULL.bits()
            | Advertisement::MODE_100_HALF.bits()
            | Advertisement::MODE_100_FULL.bits();
        assert_eq!(
            filter_advertised_10_100(all, Some(10), true, false),
            Advertisement::MODE_10_FULL.bits()
        );
        assert_eq!(
            filter_advertised_10_100(all, Some(100), true, false),
            Advertisement::MODE_10_FULL.bits() | Advertisement::MODE_100_FULL.bits()
        );
        assert_eq!(filter_advertised_10_100(all, None, false, true), 0);
        assert_eq!(
            filter_advertised_1000(
                ExtendedStatus::MODE_1000_T_HALF.bits() | ExtendedStatus::MODE_1000_T_FULL.bits(),
                None,
                true,
                false,
            ),
            GigabitControl::ADVERTISE_FULL.bits()
        );
        assert_eq!(
            filter_advertised_1000(
                ExtendedStatus::MODE_1000_T_FULL.bits(),
                Some(100),
                true,
                true
            ),
            0
        );
        assert_eq!(
            filter_advertised_1000(ExtendedStatus::MODE_1000_T_FULL.bits(), None, false, true),
            0
        );
    }

    #[kunit]
    fn page_restore_preserves_the_earliest_error() {
        assert_eq!(
            page_transaction_error(Some(SysError::DriverIncompatible), Some(SysError::Timeout)),
            Some(SysError::DriverIncompatible)
        );
        assert_eq!(
            page_transaction_error(None, Some(SysError::Timeout)),
            Some(SysError::Timeout)
        );
    }

    #[kunit]
    fn delay_readback_requires_clock_delay_and_sleep_bits() {
        let valid = DelayRegisters {
            clk_gate: DELAY_GE_TX_DIS | ClockGate::CLOCK_125MHZ.bits(),
            delay_drive: DELAY_FE_TX_DIS,
            sleep_ctrl: SleepControl::PLL_ON_IN_SLEEP.bits(),
        };
        assert!(delay_registers_match(
            valid,
            DELAY_GE_TX_DIS,
            DELAY_FE_TX_DIS
        ));
        assert!(!delay_registers_match(
            DelayRegisters {
                clk_gate: DELAY_GE_TX_DIS,
                ..valid
            },
            DELAY_GE_TX_DIS,
            DELAY_FE_TX_DIS
        ));
    }
}
