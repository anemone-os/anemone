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
    /// The target is not a symbolic link.
    NotSymlink,
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
    /// File system run out its capacity.
    NoSpace,
    /// Operation would block and nonblocking mode was requested.
    Again,
    /// Pipe write attempted after all readers were gone.
    BrokenPipe,
    /// Too many symbolic links were encountered in resolving a path.
    TooManyLinks,
    /// A symbolic link was encountered while doing a path resolution operation
    /// that does not allow symbolic links.
    LinkEncountered,
    /// Invalid path (e.g. path contains invalid UTF-8 sequences, or path is not
    /// valid for other reasons).
    InvalidPath,
    /// Device error occurred when accessing device files.
    Dev(DevError),
}

impl AsErrno for FsError {
    fn as_errno(&self) -> Errno {
        match self {
            FsError::AlreadyExists => EEXIST,
            FsError::NotFound => ENOENT,
            FsError::NotDir => ENOTDIR,
            FsError::IsDir => EISDIR,
            FsError::NotReg => EINVAL,
            FsError::NotSymlink => EINVAL,
            FsError::NotSupported => EINVAL,
            FsError::InvalidArgument => EINVAL,
            FsError::Busy => EBUSY,
            FsError::DirNotEmpty => ENOTEMPTY,
            FsError::CrossDeviceLink => EXDEV,
            FsError::NotMounted => EINVAL,
            FsError::IsMountPoint => EINVAL,
            FsError::NoMoreEntries => ENOENT,
            FsError::PermissionDenied => EPERM,
            FsError::NoSpace => ENOSPC,
            FsError::Again => EAGAIN,
            FsError::BrokenPipe => EPIPE,
            FsError::TooManyLinks => ELOOP,
            // ELOOP here might be a bit inaccurate, but POSIX actually doesn't specify the error
            // code for this case, so we choose a close enough one.
            FsError::LinkEncountered => ELOOP,
            FsError::InvalidPath => EINVAL,
            FsError::Dev(dev_err) => dev_err.as_errno(),
        }
    }
}
