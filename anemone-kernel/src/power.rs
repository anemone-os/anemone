//! System power management.

pub trait PowerArchTrait {
    /// Shut down the system.
    unsafe fn shutdown() -> !;
    /// Reboot the system.
    unsafe fn reboot() -> !;
}
