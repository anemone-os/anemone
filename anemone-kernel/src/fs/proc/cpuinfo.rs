use crate::{
    arch,
    fs::proc::pde::{ProcDirEntry, ProcDirEntryKind, ProcFileEntryOps},
    prelude::*,
};

static PROC_CPUINFO_OPS: ProcFileEntryOps = ProcFileEntryOps {
    read: arch::proc_cpuinfo_snapshot,
    write: None,
    write_at: None,
};

pub static PROC_CPUINFO_DIR_ENTRY: ProcDirEntry = ProcDirEntry {
    name: "cpuinfo",
    mode: InodeMode::new(InodeType::Regular, InodePerm::all_r()),
    kind: ProcDirEntryKind::File(&PROC_CPUINFO_OPS),
    ino: unsafe { MonoOnce::new() },
};
