use crate::{
    fs::{devfs::DevfsNode, inode::Inode, superblock::SuperBlockOps},
    prelude::*,
};

fn devfs_load_inode(sb: &Arc<SuperBlock>, ino: Ino) -> Result<Arc<Inode>, SysError> {
    unreachable!("devfs should never load inodes",);
}

fn devfs_evict_inode(_inode: Arc<Inode>) -> Result<(), SysError> {
    unreachable!("devfs inodes should never be evicted");
}

fn devfs_sync_inode(_inode: &InodeRef) -> Result<(), SysError> {
    Ok(())
}

pub(super) static DEVFS_SB_OPS: SuperBlockOps = SuperBlockOps {
    load_inode: devfs_load_inode,
    evict_inode: devfs_evict_inode,
    sync_inode: devfs_sync_inode,
};
