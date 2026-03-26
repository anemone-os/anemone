//! A simple in-memory filesystem.

use crate::{
    fs::{
        inode::Inode,
        ramfs::{
            inode::{RAMFS_DIR_INODE_OPS, RamfsDir, RamfsReg},
            superblock::{RAMFS_SB_OPS, RamfsSb},
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
fn ramfs_sb_data(sb: &SuperBlock) -> &RamfsSb {
    sb.prv()
        .cast::<RamfsSb>()
        .expect("ramfs superblock must have RamfsSb private data")
}

#[inline(always)]
fn ramfs_dir_data(inode: &InodeRef) -> Result<&RamfsDir, FsError> {
    inode
        .inode()
        .prv()
        .cast::<RamfsDir>()
        .ok_or(FsError::NotDir)
}

#[inline(always)]
fn ramfs_reg_data(inode: &InodeRef) -> Result<&RamfsReg, FsError> {
    inode
        .inode()
        .prv()
        .cast::<RamfsReg>()
        .ok_or(FsError::NotReg)
}

fn ramfs_mount(source: &MountSource, _flags: MountFlags) -> Result<MountedFileSystem, FsError> {
    if !matches!(source, MountSource::Pseudo) {
        return Err(FsError::InvalidArgument);
    }

    let sb_prv = AnyOpaque::new(RamfsSb::new());

    let fs = RAMFS.get().clone();
    let sb = Arc::new(SuperBlock::new(fs.clone(), &RAMFS_SB_OPS, sb_prv));

    fs.sget(|_| false, Some(|| sb.clone()))
        .expect("newly created superblock must be added to the file system's superblock list");

    let root_ino = Ino::try_from(1u64).unwrap();
    let root_dir_data = RamfsDir::new();

    root_dir_data.insert(".".to_string(), root_ino).unwrap();
    root_dir_data.insert("..".to_string(), root_ino).unwrap();

    let root_inode = Arc::new(Inode::new(
        root_ino,
        InodeType::Dir,
        &RAMFS_DIR_INODE_OPS,
        sb.clone(),
        AnyOpaque::new(root_dir_data),
    ));

    // though there are 3 links pointing to the root inode, we only count 2 here,
    // which is the traditional way of Unix filesystems.
    root_inode.inc_nlink();

    sb.seed_inode(root_inode);

    Ok(MountedFileSystem { sb, root_ino })
}

fn ramfs_kill_sb(sb: Arc<SuperBlock>) {
    // no-op
}

pub static RAMFS_FS_OPS: FileSystemOps = FileSystemOps {
    name: "ramfs",
    mount: ramfs_mount,
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
