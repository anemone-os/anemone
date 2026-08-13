//! Compatibility stubs for filesystem synchronization syscalls.

use crate::{prelude::*, task::files::FileDesc};

pub mod fdatasync;
pub mod fsync;
pub mod readahead;
pub mod sync;
pub mod sync_file_range;

/// The current VFS has no per-file sync operation. Keep success limited to
/// inode kinds that Linux filesystems can expose with an fsync operation;
/// other fd-backed objects must retain Linux-visible EINVAL rejection.
fn accepts_fsync_stub(file: &FileDesc) -> bool {
    !file.is_path_only()
        && matches!(
            file.vfs_file().inode().ty(),
            InodeType::Regular | InodeType::Dir | InodeType::Block
        )
}

fn accepts_sync_file_range_stub(file: &FileDesc) -> bool {
    matches!(
        file.vfs_file().inode().ty(),
        InodeType::Regular | InodeType::Dir | InodeType::Block | InodeType::Symlink
    )
}
