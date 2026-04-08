//! A simple in-memory filesystem.

use crate::{
    fs::{
        inode::Inode,
        ramfs::{
            inode::{RamfsDir, RamfsReg, RamfsSymlink, RAMFS_DIR_INODE_OPS},
            superblock::{RamfsSb, RAMFS_SB_OPS},
        },
        register_filesystem,
    },
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

mod file;
mod inode;
mod superblock;

#[inline(always)]
fn ramfs_sb(sb: &SuperBlock) -> &RamfsSb {
    sb.prv()
        .cast::<RamfsSb>()
        .expect("ramfs superblock must have RamfsSb private data")
}

#[inline(always)]
fn ramfs_dir(inode: &InodeRef) -> Result<&RamfsDir, FsError> {
    inode
        .inode()
        .prv()
        .cast::<RamfsDir>()
        .ok_or(FsError::NotDir)
}

#[inline(always)]
fn ramfs_reg(inode: &InodeRef) -> Result<&RamfsReg, FsError> {
    inode
        .inode()
        .prv()
        .cast::<RamfsReg>()
        .ok_or(FsError::NotReg)
}

#[inline(always)]
fn ramfs_symlink(inode: &InodeRef) -> Result<&RamfsSymlink, FsError> {
    inode
        .inode()
        .prv()
        .cast::<RamfsSymlink>()
        .ok_or(FsError::NotSymlink)
}

fn ramfs_mount(source: MountSource, _flags: MountFlags) -> Result<Arc<SuperBlock>, FsError> {
    if !matches!(source, MountSource::Pseudo) {
        return Err(FsError::InvalidArgument);
    }

    let sb_prv = AnyOpaque::new(RamfsSb::new());

    let fs = RAMFS.get().clone();

    let root_ino = Ino::try_from(1u64).unwrap();
    let root_dir_data = RamfsDir::new();

    root_dir_data.insert(".".to_string(), root_ino).unwrap();
    root_dir_data.insert("..".to_string(), root_ino).unwrap();

    let sb = Arc::new(SuperBlock::new(
        fs.clone(),
        &RAMFS_SB_OPS,
        sb_prv,
        root_ino,
        source,
    ));

    fs.sget(|_| false, Some(|| sb.clone()))
        .expect("newly created superblock must be added to the file system's superblock list");

    let root_inode = Arc::new(Inode::new(
        root_ino,
        InodeType::Dir,
        &RAMFS_DIR_INODE_OPS,
        sb.clone(),
        AnyOpaque::new(root_dir_data),
    ));

    // though there are 3 links pointing to the root inode, we only count 2 here,
    // which is the traditional way of Unix filesystems.
    root_inode.set_nlink(2);

    sb.seed_inode(root_inode);

    Ok(sb)
}

fn ramfs_kill_sb(sb: Arc<SuperBlock>) {}

fn ramfs_sync_fs(_sb: &SuperBlock) -> Result<(), FsError> {
    // no-op, since ramfs is purely in-memory and has no backing store to sync to.
    Ok(())
}

static RAMFS_FS_OPS: FileSystemOps = FileSystemOps {
    name: "ramfs",
    flags: FileSystemFlags::empty(),
    mount: ramfs_mount,
    sync_fs: ramfs_sync_fs,
    kill_sb: ramfs_kill_sb,
};

static RAMFS: MonoOnce<Arc<FileSystem>> = unsafe { MonoOnce::new() };

#[initcall(fs)]
fn init() {
    match register_filesystem(&RAMFS_FS_OPS) {
        Ok(fs) => RAMFS.init(|f| {
            f.write(fs);
        }),
        Err(e) => {
            kerrln!("failed to register ramfs: {:?}", e);
        },
    }
}
