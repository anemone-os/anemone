use crate::{
    fs::{devfs::DevfsNode, inode::Inode, superblock::SuperBlockOps},
    prelude::*,
};

use super::inode::devfs_new_inode;

fn devfs_load_inode(sb: &Arc<SuperBlock>, ino: Ino) -> Result<Arc<Inode>, SysError> {
    devfs_new_inode(sb.clone(), DevfsNode::new(ino)?)
}

fn devfs_evict_inode(_sb: &SuperBlock, _inode: Arc<Inode>) -> Result<(), SysError> {
    Ok(())
}

fn devfs_sync_inode(_inode: &InodeRef) -> Result<(), SysError> {
    Ok(())
}

pub(super) static DEVFS_SB_OPS: SuperBlockOps = SuperBlockOps {
    load_inode: devfs_load_inode,
    evict_inode: devfs_evict_inode,
    sync_inode: devfs_sync_inode,
};
