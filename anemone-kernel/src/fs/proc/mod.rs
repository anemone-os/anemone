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

// hooks for task-topology owned lifecycle transactions.
pub use tgid::binding::{invalidate_thread_group_binding, try_unbind_thread_group};

pub(super) fn read_snapshot_at(pos: usize, buf: &mut [u8], data: &[u8]) -> Result<usize, SysError> {
    if pos >= data.len() {
        return Ok(0);
    }

    let to_read = usize::min(buf.len(), data.len() - pos);
    buf[..to_read].copy_from_slice(&data[pos..pos + to_read]);
    Ok(to_read)
}

static PROCFS: MonoOnce<Arc<FileSystem>> = unsafe { MonoOnce::new() };

static PROCFS_SB: MonoOnce<Arc<SuperBlock>> = unsafe { MonoOnce::new() };

fn procfs_sb() -> Arc<SuperBlock> {
    PROCFS_SB.get().clone()
}

/// This function returns a vector, since procfs can be mounted multiple times.
fn procfs_root_dentries() -> Vec<Arc<Dentry>> {
    let mut dentries = vec![];
    procfs_sb().for_each_mount(|mnt| dentries.push(mnt.root().clone()));
    dentries
}

fn procfs_mount(source: MountSource, data: MountData) -> Result<Arc<SuperBlock>, SysError> {
    if !matches!(source, MountSource::Pseudo) {
        return Err(SysError::InvalidArgument);
    }
    data.reject_nonempty_for("procfs")?;

    Ok(PROCFS_SB.get().clone())
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

    // initialize singleton superblock and root inode for procfs. they will be
    // reused by all mounts of procfs.

    let fs = PROCFS.get().clone();
    let sb = Arc::new(SuperBlock::new(
        fs.clone(),
        &PROC_SB_OPS,
        NilOpaque::new(),
        PROC_ROOT_INO,
        MountSource::Pseudo,
    ));
    let root_inode = Arc::new(Inode::new(
        PROC_ROOT_INO,
        InodeType::Dir,
        &PROC_ROOT_INODE_OPS,
        sb.clone(),
        NilOpaque::new(),
    ));
    root_inode.set_meta(&InodeMeta {
        nlink: 3,
        size: 0,
        perm: InodePerm::all_rwx(),
        uid: Uid::ROOT,
        gid: Gid::ROOT,
        atime: Instant::ZERO.to_duration(),
        mtime: Instant::ZERO.to_duration(),
        ctime: Instant::ZERO.to_duration(),
    });
    sb.seed_inode(root_inode);

    PROCFS_SB.init(|slot| {
        slot.write(sb);
    });
}

// infra
mod pde;
mod root;
mod superblock;
mod sys;
mod tgid;
// TODO: mod tid;

// pdes
mod celf;
mod meminfo;
mod mounts;
mod uptime;
