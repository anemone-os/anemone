use crate::{net::NetError, prelude::*};

/// System-wide error type, wrapping errors from all subsystems (e.g. memory
/// management, device drivers, etc.).
///
/// Each subsystem defines its own `XxxError` type and implements [`AsErrno`]
/// on it. `SysError` is the common envelope used by syscall handlers to
/// propagate errors up to the ABI boundary, where [`AsErrno::as_errno`]
/// converts them to POSIX errno values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysError {
    Mm(MmError),
    Dev(DevError),
    Fs(FsError),
    Kernel(KernelError),
    Task(TaskError),
    Net(NetError),
}

/// Kernel-level errors, i.e. errors that are not specific to any subsystem, but
/// can occur in any syscall handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelError {
    /// The syscall number is invalid (i.e. no handler registered for it).
    NoSys,
    /// The syscall is valid, but not yet implemented.
    NotYetImplemented,
    /// The syscall arguments are invalid.
    InvalidArgument,
    /// The provided buffer is too small to hold the output.
    BufferTooSmall,
    /// Permission denied.
    PermissionDenied,
    /// The provided file descriptor is invalid.
    BadFileDescriptor,
    /// The file descriptor does not support seeking (e.g. socket or pipe).
    NotSeekable,
    /// The file descriptor does not refer to a directory.
    NotDirectory,
}

impl AsErrno for KernelError {
    fn as_errno(&self) -> anemone_abi::errno::Errno {
        use anemone_abi::errno::*;
        match self {
            KernelError::NoSys => ENOSYS,
            KernelError::NotYetImplemented => ENOSYS,
            KernelError::InvalidArgument => EINVAL,
            KernelError::BufferTooSmall => ERANGE,
            KernelError::PermissionDenied => EPERM,
            KernelError::BadFileDescriptor => EBADF,
            KernelError::NotSeekable => ESPIPE,
            KernelError::NotDirectory => ENOTDIR,
        }
    }
}

impl From<MmError> for SysError {
    fn from(mm_error: MmError) -> Self {
        Self::Mm(mm_error)
    }
}

impl From<DevError> for SysError {
    fn from(dev_error: DevError) -> Self {
        Self::Dev(dev_error)
    }
}

impl From<FsError> for SysError {
    fn from(fs_error: FsError) -> Self {
        Self::Fs(fs_error)
    }
}

impl From<KernelError> for SysError {
    fn from(kernel_error: KernelError) -> Self {
        Self::Kernel(kernel_error)
    }
}

impl From<NetError> for SysError {
    fn from(net_error: NetError) -> Self {
        Self::Net(net_error)
    }
}

impl AsErrno for SysError {
    fn as_errno(&self) -> anemone_abi::errno::Errno {
        match self {
            SysError::Mm(mm_error) => mm_error.as_errno(),
            SysError::Dev(dev_error) => dev_error.as_errno(),
            SysError::Fs(fs_error) => fs_error.as_errno(),
            SysError::Kernel(kernel_error) => kernel_error.as_errno(),
            SysError::Task(task_error) => task_error.as_errno(),
            SysError::Net(net_error) => net_error.as_errno(),
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
