//! Global singleton. All mounts reuse the same superblock.
//!
//!  One notable feature of procfs is that, almost every pseudo file/directory
//! has its own operation table.
//!
//! The whole procfs can be considered as a mixture of 2 parts:
//! - dynamic part: /proc/[pid] and its sub-inodes, which are generated on the
//!   fly.
//! - static part: the rest part. in linux, they are based on `struct
//!   proc_dir_entry`.

use crate::{
    fs::{
        inode::Inode,
        proc::{
            root::{PROC_ROOT_INO, PROC_ROOT_INODE_OPS},
            superblock::PROC_SB_OPS,
        },
    },
    prelude::*,
    utils::any_opaque::NilOpaque,
};

mod root;
mod superblock;
mod tgid;
// TODO: mod tid;
// TODO: mod pde;

// hook for wait-related syscalls.
pub use tgid::binding::try_unbind_thread_group;

static PROCFS: MonoOnce<Arc<FileSystem>> = unsafe { MonoOnce::new() };

/// if [None] is returned, it means procfs hasn't been mounted yet.
fn procfs_sb() -> Option<Arc<SuperBlock>> {
    // there must be no more than one superblock in procfs's sb list.
    PROCFS.get().sget(
        |sb| {
            knoticeln!("procfs_sb: found superblock");
            true
        },
        None::<fn() -> Arc<SuperBlock>>,
    )
}

/// if [None] is returned, it means procfs hasn't been mounted yet.
///
/// This function returns a vector, since procfs can be mounted multiple times.
fn procfs_root_dentries() -> Option<Vec<Arc<Dentry>>> {
    let mut dentries = vec![];
    procfs_sb().map(|sb| {
        sb.for_each_mount(|mnt| dentries.push(mnt.root().clone()));
        dentries
    })
}

fn procfs_mount(source: MountSource, flags: MountFlags) -> Result<Arc<SuperBlock>, SysError> {
    if !matches!(source, MountSource::Pseudo) {
        return Err(SysError::InvalidArgument);
    }

    let fs = PROCFS.get().clone();

    let mut new = false;

    let sb = fs
        .sget(
            |s| {
                knoticeln!("procfs mounted already, reusing existing superblock");
                true
            },
            Some(|| {
                new = true;
                Arc::new(SuperBlock::new(
                    fs.clone(),
                    &PROC_SB_OPS,
                    NilOpaque::new(),
                    PROC_ROOT_INO,
                    source,
                ))
            }),
        )
        .expect("procfs should always be able to create a superblock");

    if new {
        let root_inode = Arc::new(Inode::new(
            PROC_ROOT_INO,
            InodeType::Dir,
            &PROC_ROOT_INODE_OPS,
            sb.clone(),
            NilOpaque::new(),
        ));

        // seed the root inode.
        let root_inode = sb.seed_inode(root_inode);
    }

    Ok(sb)
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
