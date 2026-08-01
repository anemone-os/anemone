//! File descriptor management for a task.
//!
//! Reference:
//! - https://elixir.bootlin.com/linux/v6.6.32/source/include/linux/fdtable.h

mod descriptor;
mod episode;
mod opened_description;
mod table;

pub use descriptor::{FdFlags, FileDesc, FileStatusFlags, LinuxOpenCompat, OpenAccessMode};
pub use episode::FdReservation;
pub(crate) use episode::{FilesState, PosixLockBinding, PosixLockHolder};
pub use opened_description::{FileDescOps, OpenedFileFinalReleaseCtx, OpenedFileReadUserCtx};
pub(crate) use opened_description::{OpenedDescriptionCapability, OpenedDescriptionLease};
pub use table::Fd;
pub(crate) use table::FdAllocCeiling;
use table::FileTable;
