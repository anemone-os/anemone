use crate::{
    fs::proc::pde::{ProcDirEntry, ProcDirEntryKind, ProcFileEntryOps},
    mm::uspace::shm::SHMMAX,
    prelude::*,
};

fn proc_shmmax_string() -> String {
    format!("{}\n", SHMMAX)
}

static PROC_SYS_KERNEL_SHMMAX_OPS: ProcFileEntryOps = ProcFileEntryOps {
    read: proc_shmmax_string,
    write: None,
    write_at: None,
};

pub static PROC_SYS_KERNEL_SHMMAX_DIR_ENTRY: ProcDirEntry = ProcDirEntry {
    name: "shmmax",
    mode: InodeMode::new(InodeType::Regular, InodePerm::all_r()),
    kind: ProcDirEntryKind::File(&PROC_SYS_KERNEL_SHMMAX_OPS),
    ino: unsafe { MonoOnce::new() },
};
