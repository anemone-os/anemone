//! Error used throughout the memory management subsystem.

use crate::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmError {
    /// The system is out of memory.
    OutOfMemory,
    /// The virtual address is already mapped.
    AlreadyMapped,
    /// The virtual address is not mapped.
    NotMapped,
    /// The physical frame is held by multiple owners.
    SharedFrame,
    /// General invalid argument, e.g. an free operation with an invalid address
    /// or length.
    InvalidArgument,
    /// The argument is too large than the allowed maximum.
    ArgumentTooLarge,
    /// Permission denied, e.g. trying to write to a read-only page.
    PermissionDenied,
    /// The requested range is not fully covered by existing mappings.
    RangeNotMapped,
    /// The address is not properly aligned
    NotAligned,
}

impl AsErrno for MmError {
    fn as_errno(&self) -> Errno {
        match self {
            MmError::OutOfMemory => ENOMEM,
            MmError::AlreadyMapped | MmError::NotMapped | MmError::SharedFrame => EFAULT,
            MmError::InvalidArgument => EINVAL,
            MmError::PermissionDenied => EACCES,
            MmError::RangeNotMapped => ENOMEM,
            MmError::ArgumentTooLarge => E2BIG,
            MmError::NotAligned => EINVAL,
        }
    }
}
