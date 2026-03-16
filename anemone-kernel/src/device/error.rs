//! Error used throughout the device driver subsystem.

use crate::prelude::*;
use anemone_abi::errno::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevError {
    /// The device is incompatible with the driver.
    DriverIncompatible,
    /// The device is incompatible with the bus.
    BusIncompatibleDev,
    /// The driver is incompatible with the bus.
    BusIncompatibleDrv,
    /// The device is already existing.
    ExistingDevice,
    /// The driver is already existing.
    ExistingDriver,
    /// The driver is not existing.
    NoSuchDriver,
    /// The device is not existing.
    NoSuchDevice,
    /// No available resource for the driver when probing the device.
    ResourceExhausted,
    /// The device doesn't have a firmware node, but the driver requires one.
    MissingFwNode,
    /// The firmware node has no enough information to probe the device.
    FwNodeLookupFailed,
    /// No IRQ domain found for the device.
    NoIrqDomain,
    /// No interrupt information found for the device.
    NoInterruptInfo,
    /// Device has invalid interrupt information.
    InvalidInterruptInfo,
    /// The device is lacking required resources.
    MissingResource,
    /// The interrupt is unknown.
    UnknownInterrupt,
    /// The interrupt is already requested by another device.
    IrqAlreadyRequested,
    /// Failed to remap MMIO region for the device.
    IoRemapFailed(MmError),
}

impl AsErrno for DevError {
    fn as_errno(&self) -> Errno {
        // POSIX errno codes are a huge mess... it's really hard to find a perfect match
        // for each of the above error cases. fix this when we implement system calls.
        todo!()
    }
}
