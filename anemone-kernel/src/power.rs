//! System power management.

pub trait PowerArch {
    /// Shut down the system.
    unsafe fn shutdown() -> !;
    /// Reboot the system.
    unsafe fn reboot() -> !;
}
