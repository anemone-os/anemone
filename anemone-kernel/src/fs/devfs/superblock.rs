use crate::{
    fs::{
        inode::Inode,
        superblock::{FsMagic, FsStat, SuperBlockOps},
    },
    prelude::*,
};

use anemone_abi::fs::linux::stat::TMPFS_MAGIC;

fn devfs_load_inode(_sb: &Arc<SuperBlock>, _ino: Ino) -> Result<Arc<Inode>, SysError> {
    unreachable!("devfs should never load inodes")
}

fn devfs_evict_inode(_inode: Arc<Inode>) -> Result<(), SysError> {
    unreachable!("persistent devfs inodes should never be evicted")
}

fn devfs_sync_inode(_inode: &InodeRef) -> Result<(), SysError> {
    Ok(())
}

fn devfs_stat(_sb: &SuperBlock) -> Result<FsStat, SysError> {
    Ok(FsStat::pseudo(FsMagic::new(TMPFS_MAGIC)))
}

pub(super) static DEVFS_SB_OPS: SuperBlockOps = SuperBlockOps {
    load_inode: devfs_load_inode,
    evict_inode: devfs_evict_inode,
    sync_inode: devfs_sync_inode,
    stat: devfs_stat,
};
