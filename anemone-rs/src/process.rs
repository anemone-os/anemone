//! For working with processes.
//!
//! Currently we don't provide encapsulation for threads. Only single-threaded
//! processes are supported.

use anemone_abi::errno::Errno;

use crate::os::linux::process as linux_process;

/// Get current process ID.
pub fn process_id() -> usize {
    linux_process::getpid()
        .map(|x| x as usize)
        .expect("failed to invoke getpid syscall")
}

/// Exit current process.
pub fn exit(xcode: i8) -> ! {
    linux_process::exit_group(xcode)
}

/// Yield the CPU to allow other threads to run.
pub fn yield_now() -> Result<(), Errno> {
    linux_process::sched_yield()
}
