//! Translation from firmware-node properties into effective DW-MSHC limits.
//!
//! These values describe controller integration and permitted probe families;
//! they do not identify a card. The future MMC core must still issue protocol
//! commands before creating a card device.

use crate::{
    device::{bus::platform::PlatformDevice, kobject::KObject, mmc::*},
    prelude::*,
};

/// Fallback used only when firmware omits the optional standard delay.
const DEFAULT_POST_POWER_ON_DELAY_MS: u32 = 10;
/// The legacy DW-MSHC divider field accepted by the stage-1 clock model.
const MAX_DIVIDER: u32 = 255;

/// Immutable firmware snapshot consumed while constructing one host.
/// Instance-specific addresses and widths must remain sourced from the node.
pub(super) struct DwMshcFwConfig {
    pub fifo_depth: u32,
    pub data_addr: Option<usize>,
    pub ciu_clock_hz: u32,
    pub caps: MmcHostCaps,
}

impl DwMshcFwConfig {
    /// Parse and validate the subset of common/DW-MSHC properties stage 1 can
    /// honor. Missing optional exclusion properties leave that protocol family
    /// eligible; they never assert that a corresponding card is present.
    pub fn parse(device: &PlatformDevice) -> Result<Self, SysError> {
        let fwnode = device.fwnode().ok_or(SysError::MissingFwNode)?;

        let fifo_depth = match fwnode.prop_read_u32("fifo-depth") {
            Some(depth) if depth >= 2 && depth <= 4096 => depth,
            Some(depth) => {
                kerrln!("dw-mshc {}: invalid fifo-depth={}", device.name(), depth);
                return Err(SysError::InvalidArgument);
            },
            None => {
                kerrln!(
                    "dw-mshc {}: TODO(stage 1): FIFO-depth inference is currently not supported",
                    device.name()
                );
                return Err(SysError::NotYetImplemented);
            },
        };

        // Zero and absence both select the revision-derived FIFO window;
        // non-zero values are validated against the MMIO mapping later.
        let data_addr = fwnode
            .prop_read_u32("data-addr")
            .map(|value| value as usize);
        // Clock/reset owners do not exist yet. Firmware must therefore leave a
        // usable CIU rate through either the generic or assigned-rate property.
        let ciu_clock_hz = fwnode
            .prop_read_u32("clock-frequency")
            .or_else(|| fwnode.prop_read_u32("assigned-clock-rates"))
            .filter(|rate| *rate != 0)
            .ok_or_else(|| {
                kerrln!(
                    "dw-mshc {}: TODO(stage 1): clock ownership without a firmware handoff rate is currently not supported",
                    device.name()
                );
                SysError::NotYetImplemented
            })?;

        let bus_widths = decode_bus_width(fwnode.prop_read_u32("bus-width").unwrap_or(1))
            .ok_or_else(|| {
                kerrln!("dw-mshc {}: invalid bus-width", device.name());
                SysError::InvalidArgument
            })?;
        // no-* properties are a discovery allowlist projection, not a driver
        // selector and not detected-card state.
        let allowed_kinds = allowed_card_kinds(
            fwnode.prop_read_present("no-sd"),
            fwnode.prop_read_present("no-mmc"),
            fwnode.prop_read_present("no-sdio"),
        );
        if allowed_kinds.is_empty() {
            kerrln!(
                "dw-mshc {}: all card protocol kinds are disabled",
                device.name()
            );
            return Err(SysError::InvalidArgument);
        }

        let max_clock_hz = fwnode
            .prop_read_u32("max-frequency")
            .filter(|rate| *rate != 0)
            .unwrap_or(ciu_clock_hz)
            .min(ciu_clock_hz);
        let min_clock_hz = (ciu_clock_hz / (2 * MAX_DIVIDER)).max(1);
        let post_power_on_delay = Duration::from_millis(
            fwnode
                .prop_read_u32("post-power-on-delay-ms")
                .unwrap_or(DEFAULT_POST_POWER_ON_DELAY_MS) as u64,
        );

        Ok(Self {
            fifo_depth,
            data_addr,
            ciu_clock_hz,
            caps: MmcHostCaps {
                allowed_kinds,
                bus_widths,
                min_clock_hz,
                max_clock_hz,
                signal_voltages: MmcSignalVoltages::V3_3,
                // BLKSIZ and BYTCNT are modeled with their stage-1 field limits;
                // later DMA/block layers may advertise a smaller intersection.
                max_block_size: u16::MAX as u32,
                // A stop-command transaction is not part of stage 1. The
                // advertised limit prevents callers from assuming otherwise.
                max_block_count: 1,
                max_request_bytes: u16::MAX as usize,
                removable: !fwnode.prop_read_present("non-removable"),
                post_power_on_delay,
            },
        })
    }
}

fn decode_bus_width(width: u32) -> Option<MmcBusWidths> {
    match width {
        1 => Some(MmcBusWidths::ONE),
        4 => Some(MmcBusWidths::ONE | MmcBusWidths::FOUR),
        8 => Some(MmcBusWidths::ONE | MmcBusWidths::FOUR | MmcBusWidths::EIGHT),
        _ => None,
    }
}

fn allowed_card_kinds(no_sd: bool, no_mmc: bool, no_sdio: bool) -> MmcCardKinds {
    let mut kinds = MmcCardKinds::SD_MEMORY | MmcCardKinds::MMC | MmcCardKinds::SDIO;
    if no_sd {
        kinds.remove(MmcCardKinds::SD_MEMORY);
    }
    if no_mmc {
        kinds.remove(MmcCardKinds::MMC);
    }
    if no_sdio {
        kinds.remove(MmcCardKinds::SDIO);
    }
    kinds
}

#[kunit]
fn bus_width_capabilities_are_cumulative() {
    assert_eq!(decode_bus_width(1), Some(MmcBusWidths::ONE));
    assert_eq!(
        decode_bus_width(4),
        Some(MmcBusWidths::ONE | MmcBusWidths::FOUR)
    );
    assert_eq!(
        decode_bus_width(8),
        Some(MmcBusWidths::ONE | MmcBusWidths::FOUR | MmcBusWidths::EIGHT)
    );
    assert_eq!(decode_bus_width(2), None);
}

#[kunit]
fn no_properties_project_to_allowed_kinds() {
    assert_eq!(
        allowed_card_kinds(false, true, true),
        MmcCardKinds::SD_MEMORY
    );
    assert!(allowed_card_kinds(true, true, true).is_empty());
}
