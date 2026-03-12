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
    /// The physical frame is held by multiple owners.
    SharedFrame,
    /// General invalid argument, e.g. an free operation with an invalid address
    /// or length.
    InvalidArgument,
}

impl AsErrno for MmError {
    fn as_errno(&self) -> Errno {
        todo!()
    }
}
