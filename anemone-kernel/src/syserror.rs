/// System-wide error type, wrapping errors from all subsystems (e.g. memory
/// management, device drivers, etc.).
///
/// TODO
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysError {}

/// Convert a `SysError` to an `Errno` that can be returned to user space.
///
/// All subsystems should implement `AsErrno` for their error types.
pub trait AsErrno {
    fn as_errno(&self) -> anemone_abi::errno::Errno;
}
