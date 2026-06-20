use crate::{
    fs::proc::pde::{ProcDirEntry, ProcDirEntryKind, ProcFileEntryOps},
    prelude::*,
};

fn proc_tainted_string() -> String {
    // Stage-1 LTP observation surface: Anemone does not maintain Linux taint
    // bits yet, so expose an untainted kernel instead of pretending to track
    // warning/oops state. Replace this when real taint bookkeeping exists.
    "0\n".to_string()
}

static PROC_SYS_KERNEL_TAINTED_OPS: ProcFileEntryOps = ProcFileEntryOps {
    read: proc_tainted_string,
    write: None,
    write_at: None,
};

pub static PROC_SYS_KERNEL_TAINTED_DIR_ENTRY: ProcDirEntry = ProcDirEntry {
    name: "tainted",
    mode: InodeMode::new(InodeType::Regular, InodePerm::all_r()),
    kind: ProcDirEntryKind::File(&PROC_SYS_KERNEL_TAINTED_OPS),
    ino: unsafe { MonoOnce::new() },
};
