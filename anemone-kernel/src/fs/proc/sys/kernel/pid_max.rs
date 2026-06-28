use crate::{
    fs::proc::pde::{ProcDirEntry, ProcDirEntryKind, ProcFileEntryOps},
    prelude::*,
};

fn proc_pid_max_string() -> String {
    format!("{}\n", crate::kconfig_defs::MAX_PROCESSES)
}

static PROC_SYS_KERNEL_PID_MAX_OPS: ProcFileEntryOps = ProcFileEntryOps {
    read: proc_pid_max_string,
    write: None,
    write_at: None,
};

pub static PROC_SYS_KERNEL_PID_MAX_DIR_ENTRY: ProcDirEntry = ProcDirEntry {
    name: "pid_max",
    mode: InodeMode::new(InodeType::Regular, InodePerm::all_r()),
    kind: ProcDirEntryKind::File(&PROC_SYS_KERNEL_PID_MAX_OPS),
    ino: unsafe { MonoOnce::new() },
};
