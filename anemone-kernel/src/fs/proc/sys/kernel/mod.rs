use crate::{
    fs::proc::pde::{ProcDirEntry, ProcDirEntryKind},
    prelude::*,
};

use self::{
    cap_last_cap::PROC_SYS_KERNEL_CAP_LAST_CAP_DIR_ENTRY,
    pid_max::PROC_SYS_KERNEL_PID_MAX_DIR_ENTRY, shmall::PROC_SYS_KERNEL_SHMALL_DIR_ENTRY,
    shmmax::PROC_SYS_KERNEL_SHMMAX_DIR_ENTRY, shmmni::PROC_SYS_KERNEL_SHMMNI_DIR_ENTRY,
    tainted::PROC_SYS_KERNEL_TAINTED_DIR_ENTRY,
};

static PROC_SYS_KERNEL_DIR_ENTRIES: &[&ProcDirEntry] = &[
    &PROC_SYS_KERNEL_PID_MAX_DIR_ENTRY,
    &PROC_SYS_KERNEL_SHMMAX_DIR_ENTRY,
    &PROC_SYS_KERNEL_SHMALL_DIR_ENTRY,
    &PROC_SYS_KERNEL_SHMMNI_DIR_ENTRY,
    &PROC_SYS_KERNEL_CAP_LAST_CAP_DIR_ENTRY,
    &PROC_SYS_KERNEL_TAINTED_DIR_ENTRY,
];

pub static PROC_SYS_KERNEL_DIR_ENTRY: ProcDirEntry = ProcDirEntry {
    name: "kernel",
    mode: InodeMode::new(InodeType::Dir, InodePerm::all_rx()),
    kind: ProcDirEntryKind::Dir(PROC_SYS_KERNEL_DIR_ENTRIES),
    ino: unsafe { MonoOnce::new() },
};

mod cap_last_cap;
mod pid_max;
mod shmall;
mod shmmax;
mod shmmni;
mod tainted;
