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
    /// A user-provided null-terminated list or string-like array exceeded its
    /// configured bound before the terminator was found.
    ListTooLong,
    /// The provided buffer is too small to hold the output.
    BufferTooSmall,
    /// Permission denied. This includes file permission, memory access
    /// permission, etc.
    PermissionDenied,
    /// A user-provided memory address could not be accessed by the kernel.
    BadAddress,
    /// Access is denied by user-visible permission checks.
    AccessDenied,
    /// The provided file descriptor is invalid.
    BadFileDescriptor,
    /// The target file or device does not support this ioctl command.
    UnsupportedIoctl,
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
    /// The IPC identifier refers to an object that has been removed.
    IdentifierRemoved,
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
    /// File is too large.
    FileTooLarge,
    /// The directory is not empty.
    DirNotEmpty,
    /// Trying to link across different filesystems.
    CrossDeviceLink,
    /// The target filesystem is mounted read-only.
    ReadOnlyFs,
    /// Path is not a mountpoint.
    NotMounted,
    /// Path is a mountpoint.
    IsMountPoint,
    /// File system run out its capacity.
    NoSpace,
    /// Operation would block and nonblocking mode was requested.
    Again,
    /// Pipe write attempted after all readers were gone.
    BrokenPipe,
    /// The file does not support seeking.
    IllegalSeek,
    /// Too many symbolic links were encountered in resolving a path.
    TooManyLinks,
    /// A symbolic link was encountered while doing a path resolution operation
    /// that does not allow symbolic links.
    LinkEncountered,
    /// Invalid path (e.g. path contains invalid UTF-8 sequences, or path is not
    /// valid for other reasons).
    InvalidPath,
    /// A path or path component is too long.
    NameTooLong,
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
    /// The operation is interrupted by a signal.
    Interrupted,
    /// Target thread group doesn't exist.
    NoSuchProcess,
    /// Literal meaning.
    Timeout,
    /// The syscall should be restarted after interruption.
    RestartSyscall(RestartSyscall),
}

/// Indicates how to restart a syscall after interruption by a signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartSyscall {
    /// The interrupted syscall is idempotent, and has no side effect, so
    /// arguments can be safely re-applied.
    ///
    /// Examples: wait4.
    Idempotent,
    // TODO: nanosleep needs some bookkeeping in kernel to record the remaining sleep time.
}

impl SysError {
    pub const fn as_errno(&self) -> anemone_abi::errno::Errno {
        use anemone_abi::errno::*;

        match self {
            SysError::NoSys | SysError::NotYetImplemented => ENOSYS,
            SysError::NotSupported => EOPNOTSUPP,
            SysError::InvalidArgument
            | SysError::ListTooLong
            | SysError::NotReg
            | SysError::NotSymlink
            | SysError::NotMounted
            | SysError::IsMountPoint
            | SysError::InvalidPath
            | SysError::InvalidInterruptInfo
            | SysError::NotAligned => EINVAL,
            SysError::BufferTooSmall => ERANGE,
            SysError::PermissionDenied => EPERM,
            SysError::BadAddress => EFAULT,
            SysError::AccessDenied => EACCES,
            SysError::BadFileDescriptor => EBADF,
            SysError::UnsupportedIoctl => ENOTTY,
            SysError::NoMoreFd => EMFILE,
            SysError::OutOfMemory => ENOMEM,
            SysError::AlreadyMapped | SysError::NotMapped | SysError::SharedFrame => EFAULT,
            SysError::ArgumentTooLarge => E2BIG,
            SysError::RangeNotMapped => ENOMEM,
            SysError::AlreadyExists
            | SysError::ExistingDevice
            | SysError::ExistingDriver
            | SysError::DevAlreadyRegistered => EEXIST,
            SysError::NotFound => ENOENT,
            SysError::IdentifierRemoved => EIDRM,
            SysError::NotDir => ENOTDIR,
            SysError::IsDir => EISDIR,
            SysError::Busy | SysError::IrqAlreadyRequested => EBUSY,
            SysError::FileTooLarge => EFBIG,
            SysError::DirNotEmpty => ENOTEMPTY,
            SysError::CrossDeviceLink => EXDEV,
            SysError::ReadOnlyFs => EROFS,
            SysError::NoSpace | SysError::ResourceExhausted | SysError::NoMinorAvailable => ENOSPC,
            SysError::Again => EAGAIN,
            SysError::BrokenPipe => EPIPE,
            SysError::IllegalSeek => ESPIPE,
            // ELOOP here might be a bit inaccurate for TooManyLinks, but POSIX actually doesn't
            // specify the error code for this case, so we choose a close enough one.
            SysError::TooManyLinks | SysError::LinkEncountered => ELOOP,
            SysError::NameTooLong => ENAMETOOLONG,
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
            SysError::Interrupted => EINTR,
            SysError::NoSuchProcess => ESRCH,
            SysError::Timeout => ETIMEDOUT,
            SysError::RestartSyscall(_) => EINTR,
        }
    }

    pub const fn is_kernel_internal(&self) -> bool {
        match self {
            SysError::RestartSyscall(_) => true,
            _ => false,
        }
    }
}
