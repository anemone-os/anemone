//! One notable feature of procfs is that, almost every pseudo file/directory
//! has its own operation table.
//!
//! The whole procfs can be considered as a mixture of 2 parts:
//! - dynamic part: /proc/[pid] and its sub-inodes, which are generated on the
//!   fly.
//! - static part: the rest part. in linux, they are based on `struct
//!   proc_dir_entry`.

use crate::prelude::*;

mod root;
mod superblock;

static PROCFS: MonoOnce<Arc<FileSystem>> = unsafe { MonoOnce::new() };

fn procfs_mount(source: MountSource, flags: MountFlags) -> Result<Arc<SuperBlock>, SysError> {
    if !matches!(source, MountSource::Pseudo) {
        return Err(SysError::InvalidArgument);
    }

    let fs = PROCFS.get().clone();

    todo!()
}

fn procfs_sync_fs(_sb: &SuperBlock) -> Result<(), SysError> {
    // no-op.
    Ok(())
}

fn procfs_kill_sb(_sb: Arc<SuperBlock>) {
    // no-op.
}

static PROC_FS_OPS: FileSystemOps = FileSystemOps {
    name: "procfs",
    flags: FileSystemFlags::empty(),
    mount: procfs_mount,
    sync_fs: procfs_sync_fs,
    kill_sb: procfs_kill_sb,
};

#[initcall(fs)]
fn init() {
    match register_filesystem(&PROC_FS_OPS) {
        Ok(fs) => PROCFS.init(|slot| {
            slot.write(fs);
        }),
        Err(err) => {
            panic!("failed to register procfs: {:?}", err);
        },
    }
}
