//! For working with processes.

use anemone_abi::errno::Errno;

use crate::os::linux::process as linux_process;

/// Exit current process.
///
/// Currently, this is just a thin wrapper around linux's `exit` syscall, but it
/// may be extended in the future to support additional cleanup logic.
pub fn exit(xcode: i8) -> ! {
    linux_process::exit(xcode)
}

/// Yield the CPU to allow other threads to run.
pub fn yield_now() -> Result<(), Errno> {
    linux_process::sched_yield()
}
