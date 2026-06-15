use crate::{
    fs::proc::{
        pde::{ProcDirEntry, ProcDirEntryKind},
        sys::kernel::PROC_SYS_KERNEL_DIR_ENTRY,
    },
    prelude::*,
};

static PROC_SYS_DIR_ENTRIES: &[&ProcDirEntry] = &[&PROC_SYS_KERNEL_DIR_ENTRY];

pub static PROC_SYS_DIR_ENTRY: ProcDirEntry = ProcDirEntry {
    name: "sys",
    mode: InodeMode::new(InodeType::Dir, InodePerm::all_rx()),
    kind: ProcDirEntryKind::Dir(PROC_SYS_DIR_ENTRIES),
    ino: unsafe { MonoOnce::new() },
};

mod kernel;
