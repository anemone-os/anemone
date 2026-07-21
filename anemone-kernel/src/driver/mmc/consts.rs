//! Temporary MMC discovery defaults.
//!
//! These constants bridge the current cold-discovery implementation until the
//! values are discovered from the device tree. Remove them once firmware-node
//! parsing publishes the corresponding values to the MMC discovery path.

pub const MMC_IDENTIFICATION_CLOCK_HZ: u32 = 400_000;
pub const MMC_CARD_INIT_TIMEOUT_MS: u64 = 1_000;
pub const MMC_CARD_INIT_POLL_INTERVAL_MS: u64 = 10;
pub const MMC_SD_DATA_CLOCK_HZ: u32 = 25_000_000;
