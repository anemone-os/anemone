//! Motorcomm YT8511 config_init and generic Clause 22 link resolution.
//!
//! Linux 6.6's YT8511 entry only supplies `config_init`; it inherits generic
//! phylib autonegotiation and status handling. Keep that split explicit so a
//! vendor status register or board observation cannot become protocol truth.

use crate::{prelude::*, time::MonotonicInstant};

use super::regs::Dwmac1000Regs;

const PHY_ID_YT8511: u32 = 0x0000_010a;
const BMCR: u8 = 0;
const BMSR: u8 = 1;
const ADVERTISE: u8 = 4;
const LPA: u8 = 5;
const CTRL1000: u8 = 9;
const STAT1000: u8 = 10;
const ESTATUS: u8 = 15;
const BMCR_ANENABLE: u16 = 1 << 12;
const BMCR_ISOLATE: u16 = 1 << 10;
const BMCR_RESTART_AN: u16 = 1 << 9;
const BMSR_ESTATEN: u16 = 1 << 8;
const BMSR_ANEG_COMPLETE: u16 = 1 << 5;
const BMSR_ANEG_CAPABLE: u16 = 1 << 3;
const BMSR_LINK: u16 = 1 << 2;
const BMSR_10_HALF: u16 = 1 << 11;
const BMSR_10_FULL: u16 = 1 << 12;
const BMSR_100_HALF: u16 = 1 << 13;
const BMSR_100_FULL: u16 = 1 << 14;
const ESTATUS_1000_T_HALF: u16 = 1 << 12;
const ESTATUS_1000_T_FULL: u16 = 1 << 13;
const ADVERTISE_SELECTOR_MASK: u16 = 0x1f;
const ADVERTISE_CSMA: u16 = 1;
const ADVERTISE_10_HALF: u16 = 1 << 5;
const ADVERTISE_10_FULL: u16 = 1 << 6;
const ADVERTISE_100_HALF: u16 = 1 << 7;
const ADVERTISE_100_FULL: u16 = 1 << 8;
const ADVERTISE_PAUSE: u16 = 1 << 10;
const ADVERTISE_ASYM_PAUSE: u16 = 1 << 11;
const ADVERTISE_1000_HALF: u16 = 1 << 8;
const ADVERTISE_1000_FULL: u16 = 1 << 9;
const LPA_1000_HALF: u16 = 1 << 10;
const LPA_1000_FULL: u16 = 1 << 11;
const LPA_1000_MASTER_SLAVE_FAILURE: u16 = 1 << 15;
const PAGE_SELECT: u8 = 0x1e;
const PAGE_DATA: u8 = 0x1f;
const PAGE_EXT_CLK_GATE: u16 = 0x000c;
const PAGE_EXT_DELAY_DRIVE: u16 = 0x000d;
const PAGE_EXT_SLEEP_CTRL: u16 = 0x0027;
const CLK_125M: u16 = 0x0006;
const DELAY_RX: u16 = 1;
const DELAY_GE_TX_EN: u16 = 0x00f0;
const DELAY_GE_TX_DIS: u16 = 0x0020;
const DELAY_FE_TX_EN: u16 = 0xf000;
const DELAY_FE_TX_DIS: u16 = 0x2000;
const PLLON_SLP: u16 = 1 << 14;

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
) -> Result<PhyState, SysError> {
    let id1 = regs.mdio_read(address, 2)?;
    let id2 = regs.mdio_read(address, 3)?;
    if ((id1 as u32) << 16) | id2 as u32 != PHY_ID_YT8511 {
        return Err(SysError::DriverIncompatible);
    }

    // YT8511 has no driver-specific soft_reset in Linux 6.6. Its hardware
    // initialization is exactly this config_init page transaction.
    let (delay_before, delay_configured) = configure_delay(regs, address, mode)?;
    let autoneg_restarted =
        configure_generic_autoneg(regs, address, max_speed, mii, gmii, half_duplex)?;
    let link = wait_generic_link(regs, address)?;
    Ok(PhyState {
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
    })
}

fn configure_generic_autoneg(
    regs: &Dwmac1000Regs,
    address: u8,
    max_speed: Option<u32>,
    mii: bool,
    gmii: bool,
    half_duplex: bool,
) -> Result<bool, SysError> {
    let bmsr = regs.mdio_read(address, BMSR)?;
    if bmsr & BMSR_ANEG_CAPABLE == 0 {
        return Err(SysError::DriverIncompatible);
    }
    let supported_10_100 = filter_advertised_10_100(
        advertised_10_100_from_bmsr(bmsr),
        max_speed,
        mii,
        half_duplex,
    );
    let ctrl1000 = if bmsr & BMSR_ESTATEN != 0 {
        let estatus = regs.mdio_read(address, ESTATUS)?;
        let before = regs.mdio_read(address, CTRL1000)?;
        let advertised = filter_advertised_1000(estatus, max_speed, gmii, half_duplex);
        Some((
            before,
            (before & !(ADVERTISE_1000_HALF | ADVERTISE_1000_FULL)) | advertised,
        ))
    } else {
        None
    };
    if supported_10_100 == 0
        && ctrl1000
            .is_none_or(|(_, value)| value & (ADVERTISE_1000_HALF | ADVERTISE_1000_FULL) == 0)
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

    let advertise_before = regs.mdio_read(address, ADVERTISE)?;
    let advertise = (advertise_before
        & !(ADVERTISE_SELECTOR_MASK
            | ADVERTISE_10_HALF
            | ADVERTISE_10_FULL
            | ADVERTISE_100_HALF
            | ADVERTISE_100_FULL))
        | ADVERTISE_CSMA
        | supported_10_100
        // phylib supplies both pause capabilities for a normal external PHY
        // before phylink validates them against this MAC's symmetric/asymmetric
        // pause support.
        | ADVERTISE_PAUSE
        | ADVERTISE_ASYM_PAUSE;
    let mut changed = advertise != advertise_before;
    if changed {
        regs.mdio_write(address, ADVERTISE, advertise)?;
    }

    if let Some((before, configured)) = ctrl1000 {
        if configured != before {
            regs.mdio_write(address, CTRL1000, configured)?;
            changed = true;
        }
    }

    // This is Linux genphy_restart_aneg: clear isolate, enable and restart
    // Clause 22 autonegotiation after advertising supported link modes.
    let bmcr = regs.mdio_read(address, BMCR)?;
    let restart = changed || bmcr & BMCR_ANENABLE == 0 || bmcr & BMCR_ISOLATE != 0;
    if restart {
        regs.mdio_write(
            address,
            BMCR,
            (bmcr & !BMCR_ISOLATE) | BMCR_ANENABLE | BMCR_RESTART_AN,
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
        modes &= !(ADVERTISE_10_HALF | ADVERTISE_100_HALF);
    }
    match max_speed {
        Some(10) => modes & (ADVERTISE_10_HALF | ADVERTISE_10_FULL),
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
    (if half_duplex && estatus & ESTATUS_1000_T_HALF != 0 {
        ADVERTISE_1000_HALF
    } else {
        0
    }) | (if estatus & ESTATUS_1000_T_FULL != 0 {
        ADVERTISE_1000_FULL
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

fn wait_generic_link(regs: &Dwmac1000Regs, address: u8) -> Result<GenericLinkSnapshot, SysError> {
    let start = MonotonicInstant::now();
    loop {
        let bmcr = regs.mdio_read(address, BMCR)?;
        // BMSR link is latched low; generic phylib consumes two samples while
        // polling from an initially down state.
        let _ = regs.mdio_read(address, BMSR)?;
        let bmsr = regs.mdio_read(address, BMSR)?;
        if bmcr & BMCR_RESTART_AN == 0
            && bmsr & (BMSR_LINK | BMSR_ANEG_COMPLETE) == BMSR_LINK | BMSR_ANEG_COMPLETE
        {
            let advertise = regs.mdio_read(address, ADVERTISE)?;
            let lpa = regs.mdio_read(address, LPA)?;
            let ctrl1000 = regs.mdio_read(address, CTRL1000)?;
            let stat1000 = regs.mdio_read(address, STAT1000)?;
            let link = resolve_autoneg(advertise, lpa, ctrl1000, stat1000)?;
            return Ok(GenericLinkSnapshot {
                bmcr,
                bmsr,
                advertise,
                lpa,
                ctrl1000,
                stat1000,
                link,
            });
        }
        if start.elapsed() >= Duration::from_millis(DWMAC1000_PHY_TIMEOUT_MS) {
            kerrln!(
                "dwmac1000 stage=phy-link result=fail reason=timeout protocol=generic-clause22 deadline-ms={} phy-address={} bmcr={:#06x} bmsr={:#06x}",
                DWMAC1000_PHY_TIMEOUT_MS,
                address,
                bmcr,
                bmsr,
            );
            return Err(SysError::Timeout);
        }
        core::hint::spin_loop();
    }
}

const fn advertised_10_100_from_bmsr(bmsr: u16) -> u16 {
    (if bmsr & BMSR_10_HALF != 0 {
        ADVERTISE_10_HALF
    } else {
        0
    }) | (if bmsr & BMSR_10_FULL != 0 {
        ADVERTISE_10_FULL
    } else {
        0
    }) | (if bmsr & BMSR_100_HALF != 0 {
        ADVERTISE_100_HALF
    } else {
        0
    }) | (if bmsr & BMSR_100_FULL != 0 {
        ADVERTISE_100_FULL
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
    if stat1000 & LPA_1000_MASTER_SLAVE_FAILURE != 0 {
        return Err(SysError::DriverIncompatible);
    }
    let (rx_pause, tx_pause) = resolve_pause(advertise, lpa);
    let gigabit = ctrl1000 & ((stat1000 >> 2) & (ADVERTISE_1000_HALF | ADVERTISE_1000_FULL));
    if gigabit & ADVERTISE_1000_FULL != 0 {
        return Ok(PhyLink {
            speed_mbps: 1000,
            full_duplex: true,
            rx_pause,
            tx_pause,
        });
    }
    if gigabit & ADVERTISE_1000_HALF != 0 {
        return Ok(PhyLink {
            speed_mbps: 1000,
            full_duplex: false,
            rx_pause: false,
            tx_pause: false,
        });
    }
    let common = advertise & lpa;
    if common & ADVERTISE_100_FULL != 0 {
        return Ok(PhyLink {
            speed_mbps: 100,
            full_duplex: true,
            rx_pause,
            tx_pause,
        });
    }
    if common & ADVERTISE_100_HALF != 0 {
        return Ok(PhyLink {
            speed_mbps: 100,
            full_duplex: false,
            rx_pause: false,
            tx_pause: false,
        });
    }
    if common & ADVERTISE_10_FULL != 0 {
        return Ok(PhyLink {
            speed_mbps: 10,
            full_duplex: true,
            rx_pause,
            tx_pause,
        });
    }
    if common & ADVERTISE_10_HALF != 0 {
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
    const PAUSE: u16 = 1 << 10;
    const ASYM_PAUSE: u16 = 1 << 11;
    if advertise & lpa & PAUSE != 0 {
        (true, true)
    } else if advertise & lpa & ASYM_PAUSE != 0 {
        if advertise & PAUSE != 0 {
            (true, false)
        } else if lpa & PAUSE != 0 {
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
    let old_page = regs.mdio_read(address, PAGE_SELECT)?;
    let result = configure_selected_pages(regs, address, ge, fe);
    let restore = regs.mdio_write(address, PAGE_SELECT, old_page);
    finish_page_transaction(address, old_page, result, restore)
}

fn configure_selected_pages(
    regs: &Dwmac1000Regs,
    address: u8,
    ge: u16,
    fe: u16,
) -> Result<(DelayRegisters, DelayRegisters), SysError> {
    regs.mdio_write(address, PAGE_SELECT, PAGE_EXT_CLK_GATE)?;
    let clk_gate = regs.mdio_read(address, PAGE_DATA)?;
    let delay_value = modify_bits(clk_gate, DELAY_RX | DELAY_GE_TX_EN, ge);
    regs.mdio_write(address, PAGE_DATA, delay_value)?;
    regs.mdio_write(address, PAGE_DATA, delay_value | CLK_125M)?;

    regs.mdio_write(address, PAGE_SELECT, PAGE_EXT_DELAY_DRIVE)?;
    let delay_drive = regs.mdio_read(address, PAGE_DATA)?;
    regs.mdio_write(
        address,
        PAGE_DATA,
        modify_bits(delay_drive, DELAY_FE_TX_EN, fe),
    )?;

    regs.mdio_write(address, PAGE_SELECT, PAGE_EXT_SLEEP_CTRL)?;
    let sleep_ctrl = regs.mdio_read(address, PAGE_DATA)?;
    regs.mdio_write(address, PAGE_DATA, sleep_ctrl | PLLON_SLP)?;

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
    regs.mdio_write(address, PAGE_SELECT, PAGE_EXT_CLK_GATE)?;
    let clk_gate = regs.mdio_read(address, PAGE_DATA)?;
    regs.mdio_write(address, PAGE_SELECT, PAGE_EXT_DELAY_DRIVE)?;
    let delay_drive = regs.mdio_read(address, PAGE_DATA)?;
    regs.mdio_write(address, PAGE_SELECT, PAGE_EXT_SLEEP_CTRL)?;
    let sleep_ctrl = regs.mdio_read(address, PAGE_DATA)?;
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
        b"rgmii-id" => Ok((DELAY_RX | DELAY_GE_TX_EN, DELAY_FE_TX_EN)),
        _ => Err(SysError::DriverIncompatible),
    }
}

const fn delay_registers_match(registers: DelayRegisters, ge: u16, fe: u16) -> bool {
    registers.clk_gate & (DELAY_RX | DELAY_GE_TX_EN | CLK_125M) == ge | CLK_125M
        && registers.delay_drive & DELAY_FE_TX_EN == fe
        && registers.sleep_ctrl & PLLON_SLP != 0
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn rgmii_delay_modes_match_linux_motorcomm_config_init() {
        assert_eq!(delay_bits("rgmii"), Ok((DELAY_GE_TX_DIS, DELAY_FE_TX_DIS)));
        assert_eq!(
            delay_bits("rgmii-id"),
            Ok((DELAY_RX | DELAY_GE_TX_EN, DELAY_FE_TX_EN))
        );
        assert_eq!(delay_bits("sgmii"), Err(SysError::DriverIncompatible));
    }

    #[kunit]
    fn generic_resolution_prefers_fastest_common_clause22_mode() {
        assert_eq!(
            resolve_autoneg(
                ADVERTISE_1000_FULL | ADVERTISE_100_FULL,
                ADVERTISE_100_FULL,
                ADVERTISE_1000_FULL,
                LPA_1000_FULL,
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
                ADVERTISE_100_FULL | ADVERTISE_10_FULL,
                ADVERTISE_100_FULL | ADVERTISE_10_FULL,
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
            resolve_autoneg(0, 0, 0, LPA_1000_MASTER_SLAVE_FAILURE),
            Err(SysError::DriverIncompatible)
        );
    }

    #[kunit]
    fn max_speed_and_mac_capability_filter_phy_advertisement() {
        let all = ADVERTISE_10_HALF | ADVERTISE_10_FULL | ADVERTISE_100_HALF | ADVERTISE_100_FULL;
        assert_eq!(
            filter_advertised_10_100(all, Some(10), true, false),
            ADVERTISE_10_FULL
        );
        assert_eq!(
            filter_advertised_10_100(all, Some(100), true, false),
            ADVERTISE_10_FULL | ADVERTISE_100_FULL
        );
        assert_eq!(filter_advertised_10_100(all, None, false, true), 0);
        assert_eq!(
            filter_advertised_1000(ESTATUS_1000_T_HALF | ESTATUS_1000_T_FULL, None, true, false,),
            ADVERTISE_1000_FULL
        );
        assert_eq!(
            filter_advertised_1000(ESTATUS_1000_T_FULL, Some(100), true, true),
            0
        );
        assert_eq!(
            filter_advertised_1000(ESTATUS_1000_T_FULL, None, false, true),
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
            clk_gate: DELAY_GE_TX_DIS | CLK_125M,
            delay_drive: DELAY_FE_TX_DIS,
            sleep_ctrl: PLLON_SLP,
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
