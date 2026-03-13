use crate::prelude::*;

/// System-wide error type, wrapping errors from all subsystems (e.g. memory
/// management, device drivers, etc.).
///
/// TODO
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysError {
    Mm(MmError),
    Dev(DevError),
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

impl AsErrno for SysError {
    fn as_errno(&self) -> anemone_abi::errno::Errno {
        match self {
            SysError::Mm(mm_error) => mm_error.as_errno(),
            SysError::Dev(dev_error) => dev_error.as_errno(),
        }
    }
}

/// Convert a `SysError` to an `Errno` that can be returned to user space.
///
/// All subsystems should implement `AsErrno` for their error types.
pub trait AsErrno {
    fn as_errno(&self) -> anemone_abi::errno::Errno;
}
