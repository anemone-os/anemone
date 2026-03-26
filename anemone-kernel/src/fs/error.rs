//! Error used throughout the filesystem subsystem.

use crate::prelude::*;
use anemone_abi::errno::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    /// An entity with the same name already exists.
    AlreadyExists,
    /// Specified target entity (e.g. file, directory, etc.) does not exist.
    NotFound,
    /// The target is not a directory.
    NotDir,
    /// The target is a directory (but shouldn't be).
    IsDir,
    /// The target is not a regular file.
    NotReg,
    /// The operation is not supported.
    NotSupported,
    /// Invalid argument.
    InvalidArgument,
    /// The entity is busy (e.g. still has active references).
    Busy,
    /// The directory is not empty.
    DirNotEmpty,
    /// Trying to link across different filesystems.
    CrossDeviceLink,
}

impl AsErrno for FsError {
    fn as_errno(&self) -> Errno {
        todo!("do not implement this for now.")
    }
}
