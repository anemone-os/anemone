//! Translation from firmware-node properties into effective DW-MSHC limits.
//!
//! These values describe controller integration and permitted probe families;
//! they do not identify a card. The MMC discovery owner must still issue
//! protocol commands before creating a card device.

use crate::{
    device::{bus::platform::PlatformDevice, kobject::KObject, mmc::*},
    prelude::*,
};

/// Fallback used only when firmware omits the optional standard delay.
const DEFAULT_POST_POWER_ON_DELAY_MS: u32 = 10;
/// The legacy DW-MSHC divider field accepted by the synchronous clock model.
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
    /// Parse and validate the common/DW-MSHC properties honored by the current
    /// polling/PIO implementation. Missing optional exclusion properties leave
    /// that protocol family eligible; they never assert card presence.
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
                    "dw-mshc {}: TODO(stage 3+): FIFO-depth inference is currently not supported",
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
        // Firmware is the clock/reset/pinctrl owner for this polling driver.
        // It must leave a usable CIU rate through either the generic or
        // assigned-rate property. Replace this handoff only when those resource
        // providers can be acquired and sequenced by the driver itself.
        let ciu_clock_hz = fwnode
            .prop_read_u32("clock-frequency")
            .or_else(|| fwnode.prop_read_u32("assigned-clock-rates"))
            .filter(|rate| *rate != 0)
            .ok_or_else(|| {
                kerrln!(
                    "dw-mshc {}: TODO(stage 3+): clock ownership without a firmware handoff rate is currently not supported",
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

        let max_frequency = fwnode.prop_read_u32("max-frequency");
        let (min_clock_hz, max_clock_hz) =
            clock_limits(ciu_clock_hz, max_frequency).ok_or_else(|| {
                kerrln!(
                    "dw-mshc {}: invalid clock range: ciu={}Hz max-frequency={:?}",
                    device.name(),
                    ciu_clock_hz,
                    max_frequency
                );
                SysError::InvalidArgument
            })?;
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
                // BLKSIZ and BYTCNT use the current polling/PIO field limits;
                // later DMA/block layers may advertise a smaller intersection.
                max_block_size: u16::MAX as u32,
                // A stop-command transaction is not part of synchronous PIO. The
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

fn clock_limits(ciu_clock_hz: u32, max_frequency: Option<u32>) -> Option<(u32, u32)> {
    if ciu_clock_hz == 0 || max_frequency == Some(0) {
        return None;
    }
    let min_clock_hz = (ciu_clock_hz / (2 * MAX_DIVIDER)).max(1);
    let max_clock_hz = max_frequency.unwrap_or(ciu_clock_hz).min(ciu_clock_hz);
    if max_clock_hz < min_clock_hz {
        return None;
    }
    Some((min_clock_hz, max_clock_hz))
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

#[kunit]
fn firmware_clock_range_is_validated_before_capability_publication() {
    assert_eq!(clock_limits(50_000_000, None), Some((98_039, 50_000_000)));
    assert_eq!(
        clock_limits(50_000_000, Some(25_000_000)),
        Some((98_039, 25_000_000))
    );
    assert_eq!(clock_limits(50_000_000, Some(98_038)), None);
    assert_eq!(clock_limits(50_000_000, Some(0)), None);
}
