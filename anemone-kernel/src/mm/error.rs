//! Error used throughout the memory management subsystem.

use crate::prelude::*;
use anemone_abi::errno::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmError {
    /// The system is out of memory.
    OutOfMemory,
    /// The virtual address is already mapped.
    AlreadyMapped,
    /// The virtual address is not mapped.
    NotMapped,
}

impl AsErrno for MmError {
    fn as_errno(&self) -> Errno {
        match self {
            MmError::OutOfMemory => ENOMEM,
            MmError::AlreadyMapped => EALREADY,
            MmError::NotMapped => ENOENT,
        }
    }
}
