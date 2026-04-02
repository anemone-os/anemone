use crate::prelude::*;

/// System-wide error type, wrapping errors from all subsystems (e.g. memory
/// management, device drivers, etc.).
///
/// TODO
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysError {
    Mm(MmError),
    Dev(DevError),
    Fs(FsError),
    Kernel(KernelError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelError {
    /// The syscall number is invalid (i.e. no handler registered for it).
    NoSys,
    /// The syscall is valid, but not yet implemented.
    NotYetImplemented,
    /// The syscall arguments are invalid.
    InvalidArgument,
}

impl AsErrno for KernelError {
    fn as_errno(&self) -> anemone_abi::errno::Errno {
        use anemone_abi::errno::*;
        match self {
            KernelError::NoSys => ENOSYS,
            KernelError::NotYetImplemented => ENOSYS,
            KernelError::InvalidArgument => EINVAL,
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

impl AsErrno for SysError {
    fn as_errno(&self) -> anemone_abi::errno::Errno {
        match self {
            SysError::Mm(mm_error) => mm_error.as_errno(),
            SysError::Dev(dev_error) => dev_error.as_errno(),
            SysError::Fs(fs_error) => fs_error.as_errno(),
            SysError::Kernel(kernel_error) => kernel_error.as_errno(),
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
