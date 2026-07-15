//! Protocol-neutral MMC host infrastructure.
//!
//! This module models host-controller capabilities and requests. Card
//! discovery and SD/eMMC/SDIO protocol policy intentionally live above this
//! layer and are not part of the infrastructure stage.

mod host;
mod registry;

pub use host::*;
pub use registry::*;
