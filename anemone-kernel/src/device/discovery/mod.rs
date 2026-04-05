//! Platform Discovery
//!
//! Each platform may have different hardware components and configurations. The
//! `PlatformDiscovery` trait provides an interface for discovering devices,
//! memory layout, and other platform-specific information. This allows the
//! kernel to adapt to different platforms without hardcoding platform-specific
//! details.
//!
//! On x86_64, we can use ACPI, while on some embedded platforms such as ARM and
//! RISC-V, Device Tree may be used.

use crate::initcall::{InitCallLevel, run_initcalls};

/// Currently this trait is unused.
///
/// When we need to support more discovery mechanisms, we may abstract them
/// behind this trait.
pub trait PlatformDiscovery {}

pub mod fwnode;
pub mod open_firmware;

/// Probe those virtual devices. e.g. null, zero, ramdisk, etc.
///
/// This function should be called right after physical device discovery.
pub unsafe fn probe_virtual_devices() {
    unsafe {
        run_initcalls(InitCallLevel::Probe);
    }
}
