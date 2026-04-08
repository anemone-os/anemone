//! For working with processes.
//!
//! It's not recommended to call these thin wrappers around linux syscalls
//! directly. Instead, prefer upper-level os-agnostic encapsulations.

use anemone_abi::errno::Errno;

use crate::os::linux::process as linux_process;

/// Exit current process.
///
/// Currently, this is just a thin wrapper around linux's `exit` syscall, but it
/// may be extended in the future to support other platforms or additional
/// cleanup logic.
pub fn exit(xcode: i32) -> ! {
    linux_process::exit(xcode)
}

/// Yield the CPU to allow other threads to run.
pub fn yield_now() -> Result<(), Errno> {
    linux_process::sched_yield()
}
