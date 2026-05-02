//! TODO: Eliminate redundancy and refine overly broad descriptions.

/// System-wide error type.
///
/// TODO: many error types, such as those driver/device related errors, should
/// be handled in kernel itself, rather than being exposed to user space. We
/// should refine this later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysError {
    /// The syscall number is invalid (i.e. no handler registered for it).
    NoSys,
    /// The functionality is not yet implemented.
    NotYetImplemented,
    /// The functionality is not supported.
    ///
    /// The difference between `NotYetImplemented` and `NotSupported` is that
    /// the former is used for functionality that is planned to be implemented
    /// in the future, while the latter is used for functionality that is not
    /// planned to be implemented.
    NotSupported,
    /// The syscall arguments are invalid.
    InvalidArgument,
    /// The provided buffer is too small to hold the output.
    BufferTooSmall,
    /// Permission denied. This includes file permission, memory access
    /// permission, etc.
    PermissionDenied,
    /// The provided file descriptor is invalid.
    BadFileDescriptor,
    /// No more file descriptors available for allocation.
    NoMoreFd,
    /// System or current process is out of memory.
    OutOfMemory,
    /// The virtual address is already mapped.
    AlreadyMapped,
    /// The virtual address is not mapped.
    NotMapped,
    /// The physical frame is held by multiple owners.
    SharedFrame,
    /// Argument list is too long. Used when constructing initial process stack,
    /// for example.
    ArgumentTooLarge,
    /// The requested range is not fully covered by existing mappings.
    RangeNotMapped,
    /// The address is not properly aligned.
    NotAligned,
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
    /// The device number is already registered in current subsystem.
    DevAlreadyRegistered,
    /// No available minor number for the device.
    NoMinorAvailable,
    /// General probe failure.
    ProbeFailed,
    /// Indicates an I/O error occurred.
    IO,
    /// Unexpected end of file.
    UnexpectedEof,
    /// Child process is not found.
    ChildNotFound,
    /// Binary format unrecognized.
    BinFmtUnrecognized,
}

impl AsErrno for SysError {
    fn as_errno(&self) -> anemone_abi::errno::Errno {
        use anemone_abi::errno::*;

        match self {
            SysError::NoSys | SysError::NotYetImplemented => ENOSYS,
            SysError::NotSupported => EOPNOTSUPP,
            SysError::InvalidArgument
            | SysError::NotReg
            | SysError::NotSymlink
            | SysError::NotMounted
            | SysError::IsMountPoint
            | SysError::InvalidPath
            | SysError::InvalidInterruptInfo
            | SysError::NotAligned => EINVAL,
            SysError::BufferTooSmall => ERANGE,
            SysError::PermissionDenied => EPERM,
            SysError::BadFileDescriptor => EBADF,
            SysError::NoMoreFd => EMFILE,
            SysError::OutOfMemory => ENOMEM,
            SysError::AlreadyMapped | SysError::NotMapped | SysError::SharedFrame => EFAULT,
            SysError::ArgumentTooLarge => E2BIG,
            SysError::RangeNotMapped => ENOMEM,
            SysError::AlreadyExists
            | SysError::ExistingDevice
            | SysError::ExistingDriver
            | SysError::DevAlreadyRegistered => EEXIST,
            SysError::NotFound | SysError::NoMoreEntries => ENOENT,
            SysError::NotDir => ENOTDIR,
            SysError::IsDir => EISDIR,
            SysError::Busy | SysError::IrqAlreadyRequested => EBUSY,
            SysError::DirNotEmpty => ENOTEMPTY,
            SysError::CrossDeviceLink => EXDEV,
            SysError::NoSpace | SysError::ResourceExhausted | SysError::NoMinorAvailable => ENOSPC,
            SysError::Again => EAGAIN,
            SysError::BrokenPipe => EPIPE,
            // ELOOP here might be a bit inaccurate for TooManyLinks, but POSIX actually doesn't
            // specify the error code for this case, so we choose a close enough one.
            SysError::TooManyLinks | SysError::LinkEncountered => ELOOP,
            SysError::DriverIncompatible
            | SysError::BusIncompatibleDev
            | SysError::BusIncompatibleDrv
            | SysError::NoSuchDriver
            | SysError::NoSuchDevice
            | SysError::MissingFwNode
            | SysError::FwNodeLookupFailed
            | SysError::NoIrqDomain
            | SysError::NoInterruptInfo
            | SysError::MissingResource
            | SysError::UnknownInterrupt
            | SysError::ProbeFailed => ENODEV,
            SysError::IO => EIO,
            SysError::UnexpectedEof => ENODATA,
            SysError::ChildNotFound => ECHILD,
            SysError::BinFmtUnrecognized => ENOEXEC,
        }
    }
}

/// Convert a `SysError` to an `Errno` that can be returned to user space.
///
/// All subsystems should implement `AsErrno` for their error types.
pub trait AsErrno {
    #[track_caller]
    fn as_errno(&self) -> anemone_abi::errno::Errno;
}
