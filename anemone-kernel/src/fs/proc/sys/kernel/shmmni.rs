use crate::{
    fs::proc::pde::{ProcDirEntry, ProcDirEntryKind, ProcFileEntryOps},
    mm::uspace::shm::SHMMNI,
    prelude::*,
};

fn proc_shmmni_string() -> String {
    format!("{}\n", SHMMNI)
}

static PROC_SYS_KERNEL_SHMMNI_OPS: ProcFileEntryOps = ProcFileEntryOps {
    read: proc_shmmni_string,
    write: None,
    write_at: None,
};

pub static PROC_SYS_KERNEL_SHMMNI_DIR_ENTRY: ProcDirEntry = ProcDirEntry {
    name: "shmmni",
    mode: InodeMode::new(InodeType::Regular, InodePerm::all_r()),
    kind: ProcDirEntryKind::File(&PROC_SYS_KERNEL_SHMMNI_OPS),
    ino: unsafe { MonoOnce::new() },
};
