use crate::{
    fs::proc::pde::{ProcDirEntry, ProcDirEntryKind, ProcFileEntryOps},
    mm::uspace::shm::SHMALL,
    prelude::*,
};

fn proc_shmall_string() -> String {
    format!("{}\n", SHMALL)
}

static PROC_SYS_KERNEL_SHMALL_OPS: ProcFileEntryOps = ProcFileEntryOps {
    read: proc_shmall_string,
    write: None,
    write_at: None,
};

pub static PROC_SYS_KERNEL_SHMALL_DIR_ENTRY: ProcDirEntry = ProcDirEntry {
    name: "shmall",
    mode: InodeMode::new(InodeType::Regular, InodePerm::all_r()),
    kind: ProcDirEntryKind::File(&PROC_SYS_KERNEL_SHMALL_OPS),
    ino: unsafe { MonoOnce::new() },
};
