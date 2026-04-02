//! Error used throughout the filesystem subsystem.

use crate::prelude::*;

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
    /// Path is not a mountpoint.
    NotMounted,
    /// Path is a mountpoint.
    IsMountPoint,
    /// No more entries to iterate (used by `iterate` file operation).
    NoMoreEntries,
    /// Permission denied.
    PermissionDenied,
}

impl AsErrno for FsError {
    fn as_errno(&self) -> Errno {
        match self {
            FsError::AlreadyExists => EEXIST,
            FsError::NotFound => ENOENT,
            FsError::NotDir => ENOTDIR,
            FsError::IsDir => EISDIR,
            FsError::NotReg => EINVAL,
            FsError::NotSupported => EINVAL,
            FsError::InvalidArgument => EINVAL,
            FsError::Busy => EBUSY,
            FsError::DirNotEmpty => ENOTEMPTY,
            FsError::CrossDeviceLink => EXDEV,
            FsError::NotMounted => EINVAL,
            FsError::IsMountPoint => EINVAL,
            FsError::NoMoreEntries => ENOENT,
            FsError::PermissionDenied => EPERM,
        }
    }
}
