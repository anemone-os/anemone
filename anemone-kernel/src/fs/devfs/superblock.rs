use crate::{
    fs::{devfs::DEVFS_ROOT_INO, inode::Inode, superblock::SuperBlockOps},
    prelude::*,
};

fn devfs_load_inode(_sb: &Arc<SuperBlock>, _ino: Ino) -> Result<Arc<Inode>, SysError> {
    unreachable!("devfs should never load inodes")
}

fn devfs_evict_inode(_inode: Arc<Inode>) -> Result<(), SysError> {
    unreachable!("persistent devfs inodes should never be evicted")
}

fn devfs_sync_inode(_inode: &InodeRef) -> Result<(), SysError> {
    Ok(())
}

pub(super) fn alloc_ino() -> Ino {
    static INO_ALLOC: AtomicU64 = AtomicU64::new(DEVFS_ROOT_INO.get() + 1);

    loop {
        let current = INO_ALLOC.load(Ordering::Acquire);
        let next = current.checked_add(1).expect("devfs inode number overflow");
        if INO_ALLOC
            .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Ino::new(current);
        }
    }
}

pub(super) static DEVFS_SB_OPS: SuperBlockOps = SuperBlockOps {
    load_inode: devfs_load_inode,
    evict_inode: devfs_evict_inode,
    sync_inode: devfs_sync_inode,
};
