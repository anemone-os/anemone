use crate::{
	fs::{inode::Inode, superblock::SuperBlockOps},
	prelude::*,
};

use super::{devfs_node_from_ino, inode::devfs_new_inode};

fn devfs_load_inode(sb: &Arc<SuperBlock>, ino: Ino) -> Result<Arc<Inode>, FsError> {
	let node = devfs_node_from_ino(ino)?;
	devfs_new_inode(sb.clone(), node)
}

fn devfs_evict_inode(_sb: &SuperBlock, _inode: Arc<Inode>) -> Result<(), FsError> {
	Ok(())
}

fn devfs_sync_inode(_inode: &InodeRef) -> Result<(), FsError> {
	Ok(())
}

pub(super) static DEVFS_SB_OPS: SuperBlockOps = SuperBlockOps {
	load_inode: devfs_load_inode,
	evict_inode: devfs_evict_inode,
	sync_inode: devfs_sync_inode,
};
