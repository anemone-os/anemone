use crate::{
    fs::proc::pde::{ProcDirEntry, ProcDirEntryKind, ProcSymlinkEntryOps},
    prelude::*,
};

fn proc_mounts_read_link() -> PathBuf {
    PathBuf::from("self/mounts")
}

static PROC_MOUNTS_SYMLINK_OPS: ProcSymlinkEntryOps = ProcSymlinkEntryOps {
    target: proc_mounts_read_link,
};

pub static PROC_MOUNTS_DIR_ENTRY: ProcDirEntry = ProcDirEntry {
    name: "mounts",
    mode: InodeMode::new(InodeType::Symlink, InodePerm::all_rwx()),
    kind: ProcDirEntryKind::Symlink(&PROC_MOUNTS_SYMLINK_OPS),
    ino: unsafe { MonoOnce::new() },
};
