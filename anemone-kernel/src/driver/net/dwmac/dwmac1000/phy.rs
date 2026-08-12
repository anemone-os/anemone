//! Bounded Motorcomm YT8511 Clause 22/P1 transaction.
//!
//! Linux 6.6's motorcomm driver identifies YT8511 as 0x0000010a and uses the
//! page-selected registers below for RGMII delay and clock setup. This module
//! keeps that vendor protocol private to the DWMAC1000 Gate 2 probe.

use crate::{prelude::*, time::MonotonicInstant};

use super::regs::Dwmac1000Regs;

const PHY_ID_YT8511: u32 = 0x0000_010a;
const BMCR: u8 = 0;
const BMSR: u8 = 1;
const BMCR_RESET: u16 = 1 << 15;
const BMCR_ANENABLE: u16 = 1 << 12;
const BMCR_POWER_DOWN: u16 = 1 << 11;
const BMCR_ISOLATE: u16 = 1 << 10;
const BMCR_RESTART_AN: u16 = 1 << 9;
const BMSR_ANEG_COMPLETE: u16 = 1 << 5;
const BMSR_LINK: u16 = 1 << 2;
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

#[derive(Debug, Clone, Copy)]
pub(super) struct PhyState {
    pub(super) bmcr_before_reset: u16,
    pub(super) bmcr_after_reset: u16,
    pub(super) bmcr: u16,
    pub(super) bmsr: u16,
    pub(super) link: bool,
    pub(super) delay_before_reset: DelayRegisters,
    pub(super) delay_after_reset: DelayRegisters,
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
) -> Result<PhyState, SysError> {
    let id1 = regs.mdio_read(address, 2)?;
    let id2 = regs.mdio_read(address, 3)?;
    let phy_id = ((id1 as u32) << 16) | id2 as u32;
    if phy_id != PHY_ID_YT8511 {
        return Err(SysError::DriverIncompatible);
    }
    let bmcr_before_reset = regs.mdio_read(address, BMCR)?;
    let delay_before_reset = read_delay_registers(regs, address)?;
    regs.mdio_write(address, BMCR, bmcr_before_reset | BMCR_RESET)?;
    let bmcr_after_reset = wait_reset(regs, address)?;
    let (delay_after_reset, delay_configured) = configure_delay(regs, address, mode)?;
    let bmcr = regs.mdio_read(address, BMCR)?;
    let restarted = (bmcr & !(BMCR_POWER_DOWN | BMCR_ISOLATE)) | BMCR_ANENABLE | BMCR_RESTART_AN;
    regs.mdio_write(address, BMCR, restarted)?;
    let (bmcr, bmsr, link) = wait_link(regs, address)?;
    Ok(PhyState {
        bmcr_before_reset,
        bmcr_after_reset,
        bmcr,
        bmsr,
        link,
        delay_before_reset,
        delay_after_reset,
        delay_configured,
    })
}

fn wait_reset(regs: &Dwmac1000Regs, address: u8) -> Result<u16, SysError> {
    let start = MonotonicInstant::now();
    loop {
        let bmcr = regs.mdio_read(address, BMCR)?;
        if bmcr & BMCR_RESET == 0 {
            return Ok(bmcr);
        }
        if start.elapsed() >= Duration::from_millis(DWMAC1000_MDIO_TIMEOUT_MS) {
            kerrln!(
                "dwmac1000 stage=phy-reset result=fail reason=timeout phy-address={} deadline-ms={} bmcr-last={:#06x}",
                address,
                DWMAC1000_MDIO_TIMEOUT_MS,
                bmcr,
            );
            return Err(SysError::Timeout);
        }
        core::hint::spin_loop();
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
    // PAGE_SELECT is shared PHY state. Restore it before propagating either a
    // transaction failure or a restore failure to the caller.
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
    let clock_value = modify_bits(delay_value, 0, CLK_125M);
    regs.mdio_write(address, PAGE_DATA, clock_value)?;

    regs.mdio_write(address, PAGE_SELECT, PAGE_EXT_DELAY_DRIVE)?;
    let delay_drive = regs.mdio_read(address, PAGE_DATA)?;
    regs.mdio_write(
        address,
        PAGE_DATA,
        modify_bits(delay_drive, DELAY_FE_TX_EN, fe),
    )?;

    regs.mdio_write(address, PAGE_SELECT, PAGE_EXT_SLEEP_CTRL)?;
    let sleep_ctrl = regs.mdio_read(address, PAGE_DATA)?;
    regs.mdio_write(address, PAGE_DATA, modify_bits(sleep_ctrl, 0, PLLON_SLP))?;

    let before = DelayRegisters {
        clk_gate,
        delay_drive,
        sleep_ctrl,
    };
    // Vendor writes are fail-forward inside this bounded probe. If a later
    // write fails, the next PHY reset/reprobe is the recovery boundary.
    let after = read_selected_delay_registers(regs, address)?;
    if !delay_registers_match(after, ge, fe) {
        kerrln!(
            "dwmac1000 stage=phy-delay result=fail reason=readback-mismatch phy-address={} page0c-before={:#06x} page0c-after={:#06x} page0d-before={:#06x} page0d-after={:#06x} page27-before={:#06x} page27-after={:#06x}",
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

fn read_delay_registers(regs: &Dwmac1000Regs, address: u8) -> Result<DelayRegisters, SysError> {
    let old_page = regs.mdio_read(address, PAGE_SELECT)?;
    let result = read_selected_delay_registers(regs, address);
    let restore = regs.mdio_write(address, PAGE_SELECT, old_page);
    finish_page_transaction(address, old_page, result, restore)
}

fn finish_page_transaction<T>(
    address: u8,
    old_page: u16,
    operation: Result<T, SysError>,
    restore: Result<(), SysError>,
) -> Result<T, SysError> {
    match (operation, restore) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => {
            kerrln!(
                "dwmac1000 stage=phy-page result=fail phy-address={} operation-error={:?} cleanup=restored old-page={:#06x} recovery=phy-reset-or-reprobe",
                address,
                error,
                old_page,
            );
            Err(error)
        },
        (Ok(_), Err(restore_error)) => {
            kerrln!(
                "dwmac1000 stage=phy-page result=fail phy-address={} operation-error=none cleanup=restore-failed old-page={:#06x} restore-error={:?} recovery=phy-reset-or-reprobe",
                address,
                old_page,
                restore_error,
            );
            Err(restore_error)
        },
        (Err(error), Err(restore_error)) => {
            kerrln!(
                "dwmac1000 stage=phy-page result=fail phy-address={} operation-error={:?} cleanup=restore-failed old-page={:#06x} restore-error={:?} recovery=phy-reset-or-reprobe",
                address,
                error,
                old_page,
                restore_error,
            );
            Err(error)
        },
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

fn wait_link(regs: &Dwmac1000Regs, address: u8) -> Result<(u16, u16, bool), SysError> {
    let start = MonotonicInstant::now();
    loop {
        let bmcr = regs.mdio_read(address, BMCR)?;
        // BMSR link is latched low. The second read is the current snapshot.
        let _ = regs.mdio_read(address, BMSR)?;
        let bmsr = regs.mdio_read(address, BMSR)?;
        let restarting = bmcr & BMCR_RESTART_AN != 0;
        let autoneg_incomplete = bmcr & BMCR_ANENABLE != 0 && bmsr & BMSR_ANEG_COMPLETE == 0;
        if !restarting && !autoneg_incomplete && bmsr & BMSR_LINK != 0 {
            return Ok((bmcr, bmsr, true));
        }
        if start.elapsed() >= Duration::from_millis(DWMAC1000_MDIO_TIMEOUT_MS) {
            return Ok((bmcr, bmsr, false));
        }
        core::hint::spin_loop();
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn rgmii_disables_internal_delays_and_keeps_required_clock_bits() {
        let configured = DelayRegisters {
            clk_gate: modify_bits(
                u16::MAX,
                DELAY_RX | DELAY_GE_TX_EN,
                DELAY_GE_TX_DIS | CLK_125M,
            ),
            delay_drive: modify_bits(u16::MAX, DELAY_FE_TX_EN, DELAY_FE_TX_DIS),
            sleep_ctrl: modify_bits(0, 0, PLLON_SLP),
        };
        assert!(delay_registers_match(
            configured,
            DELAY_GE_TX_DIS,
            DELAY_FE_TX_DIS
        ));
        assert_eq!(configured.clk_gate & DELAY_RX, 0);
    }

    #[kunit]
    fn rgmii_id_enables_both_internal_delays() {
        let ge = DELAY_RX | DELAY_GE_TX_EN;
        let configured = DelayRegisters {
            clk_gate: modify_bits(0, DELAY_RX | DELAY_GE_TX_EN, ge | CLK_125M),
            delay_drive: modify_bits(0, DELAY_FE_TX_EN, DELAY_FE_TX_EN),
            sleep_ctrl: modify_bits(0, 0, PLLON_SLP),
        };
        assert!(delay_registers_match(configured, ge, DELAY_FE_TX_EN));
    }

    #[kunit]
    fn readback_validation_rejects_missing_required_bits() {
        assert!(!delay_registers_match(
            DelayRegisters {
                clk_gate: DELAY_GE_TX_DIS,
                delay_drive: DELAY_FE_TX_DIS,
                sleep_ctrl: PLLON_SLP,
            },
            DELAY_GE_TX_DIS,
            DELAY_FE_TX_DIS,
        ));
    }

    #[kunit]
    fn only_board_declared_rgmii_modes_are_supported() {
        assert_eq!(delay_bits("rgmii"), Ok((DELAY_GE_TX_DIS, DELAY_FE_TX_DIS)));
        assert_eq!(
            delay_bits("rgmii-id"),
            Ok((DELAY_RX | DELAY_GE_TX_EN, DELAY_FE_TX_EN))
        );
        assert_eq!(delay_bits("rgmii-rxid"), Err(SysError::DriverIncompatible));
        assert_eq!(delay_bits("rgmii-txid"), Err(SysError::DriverIncompatible));
    }
}
