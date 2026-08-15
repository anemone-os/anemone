//! Read-only `/proc/nemophila` runtime projection.

mod dir;
mod instance;

use crate::{
    fs::proc::pde::{ProcDirEntry, ProcDirEntryKind},
    prelude::*,
};

pub static PROC_NEMOPHILA_DIR_ENTRY: ProcDirEntry = ProcDirEntry {
    name: "nemophila",
    mode: InodeMode::new(InodeType::Dir, InodePerm::all_rx()),
    kind: ProcDirEntryKind::Custom(&dir::PROC_NEMOPHILA_DIR_INODE_OPS),
    ino: unsafe { MonoOnce::new() },
};

fn readonly_attr(inode: &InodeRef, nlink: u64, size: u64) -> Result<InodeStat, SysError> {
    let meta = inode.inode().meta_snapshot();
    let now = RealtimeInstant::now().to_duration();
    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: inode.mode(),
        nlink,
        uid: meta.uid,
        gid: meta.gid,
        rdev: DeviceId::None,
        size,
        atime: now,
        mtime: now,
        ctime: now,
    })
}
