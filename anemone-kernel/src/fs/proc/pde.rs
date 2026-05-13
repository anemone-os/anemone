//! Roughly the `struct proc_dir_entry` in Linux.

use crate::{
    fs::{
        inode::Inode,
        proc::{
            celf::PROC_SELF_DIR_ENTRY, mounts::PROC_MOUNTS_DIR_ENTRY, procfs_sb,
            superblock::alloc_ino, uptime::PROC_UPTIME_DIR_ENTRY,
        },
    },
    prelude::*,
    utils::any_opaque::NilOpaque,
};

pub struct ProcDirEntry {
    pub name: &'static str,
    pub mode: InodeMode,
    pub ops: &'static InodeOps,
    /// Pseudo inodes should always leave this field [MonoOnce::new], and real
    /// inode numbers will be allocated during probe initcall and stored here.
    pub ino: MonoOnce<Ino>,
}

static PROC_DIR_ENTRIES: &[&ProcDirEntry] = &[
    &PROC_UPTIME_DIR_ENTRY,
    &PROC_SELF_DIR_ENTRY,
    &PROC_MOUNTS_DIR_ENTRY,
    // TODO: mounts, interrupts, version, devices, kallsyms, etc.
];

pub fn find_pde_by_name(name: &str) -> Option<&'static ProcDirEntry> {
    for &pde in PROC_DIR_ENTRIES {
        if pde.name == name {
            return Some(pde);
        }
    }
    None
}

// TODO: create a new `late_init` for this?
#[initcall(probe)]
fn init() {
    let sb = procfs_sb();

    for ProcDirEntry {
        name,
        mode,
        ops,
        ino,
    } in PROC_DIR_ENTRIES
    {
        let inode = Inode::new(alloc_ino(), mode.ty(), ops, sb.clone(), NilOpaque::new());
        inode.set_meta(&InodeMeta {
            nlink: if mode.ty() == InodeType::Dir { 3 } else { 1 },
            size: 0,
            perm: mode.perm(),
            atime: Instant::ZERO.to_duration(),
            mtime: Instant::ZERO.to_duration(),
            ctime: Instant::ZERO.to_duration(),
        });
        ino.init(|slot| {
            slot.write(inode.ino());
        });
        let _inode = sb.seed_inode(Arc::new(inode));

        kdebugln!("procfs: registered pde {} with ino {}", name, _inode.ino());
    }
}
