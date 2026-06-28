use anemone_abi::capability::linux::CAP_LAST_CAP;

use crate::{
    fs::proc::pde::{ProcDirEntry, ProcDirEntryKind, ProcFileEntryOps},
    prelude::*,
};

fn proc_cap_last_cap_string() -> String {
    format!("{}\n", CAP_LAST_CAP)
}

static PROC_SYS_KERNEL_CAP_LAST_CAP_OPS: ProcFileEntryOps = ProcFileEntryOps {
    read: proc_cap_last_cap_string,
    write: None,
    write_at: None,
};

pub static PROC_SYS_KERNEL_CAP_LAST_CAP_DIR_ENTRY: ProcDirEntry = ProcDirEntry {
    name: "cap_last_cap",
    mode: InodeMode::new(InodeType::Regular, InodePerm::all_r()),
    kind: ProcDirEntryKind::File(&PROC_SYS_KERNEL_CAP_LAST_CAP_OPS),
    ino: unsafe { MonoOnce::new() },
};
