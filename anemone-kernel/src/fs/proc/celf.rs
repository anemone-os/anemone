//! 'celf' instead of 'self' to avoid collision with `self` keyword.

use crate::{
    fs::proc::pde::{ProcDirEntry, ProcDirEntryKind, ProcSymlinkEntryOps},
    prelude::*,
};

fn proc_self_read_link() -> PathBuf {
    let tgid = get_current_task().tgid().get();

    PathBuf::from(&tgid.to_string())
}

static PROC_SELF_SYMLINK_OPS: ProcSymlinkEntryOps = ProcSymlinkEntryOps {
    target: proc_self_read_link,
};

pub static PROC_SELF_DIR_ENTRY: ProcDirEntry = ProcDirEntry {
    name: "self",
    mode: InodeMode::new(InodeType::Symlink, InodePerm::all_rwx()),
    kind: ProcDirEntryKind::Symlink(&PROC_SELF_SYMLINK_OPS),
    ino: unsafe { MonoOnce::new() },
};
