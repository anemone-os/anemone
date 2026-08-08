use crate::{
    fs::{
        inode::Inode,
        superblock::{FsMagic, FsStat, SuperBlockOps},
    },
    prelude::*,
};

use anemone_abi::fs::linux::stat::RAMFS_MAGIC;

#[derive(Opaque)]
pub(super) struct RamfsSb {
    next_ino: AtomicU64,
    /// Sleeping gate for ramfs namespace transactions. `RamfsDir::children`
    /// protects each directory container, while this lock keeps lookup-to-iget
    /// and compound inode-cache/link-count/dirent updates in one transaction.
    tx_lock: Mutex<()>,
}

impl RamfsSb {
    pub(super) fn new() -> Self {
        Self {
            next_ino: AtomicU64::new(2), // Ino 0 is reserved; root gets ino 1; children start at 2.
            tx_lock: Mutex::new(()),
        }
    }

    pub(super) fn alloc_ino(&self) -> Ino {
        Ino::try_from(self.next_ino.fetch_add(1, Ordering::Relaxed)).unwrap()
    }

    pub(super) fn with_tx<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.tx_lock.lock();
        f()
    }
}

// ramfs has no backing store, a cache miss simply means the inode doesn't
// exist.
fn ramfs_load_inode(_sb: &Arc<SuperBlock>, _ino: Ino) -> Result<Arc<Inode>, SysError> {
    Err(SysError::NotFound)
}

fn ramfs_evict_inode(_inode: Arc<Inode>) -> Result<(), SysError> {
    // the same as sync_inode.
    Ok(())
}

fn ramfs_sync_inode(_inode: &InodeRef) -> Result<(), SysError> {
    // ramfs has nothing to do here, since we don't have a backing store to write
    // back
    Ok(())
}

fn ramfs_stat(_sb: &SuperBlock) -> Result<FsStat, SysError> {
    Ok(FsStat::pseudo(FsMagic::new(RAMFS_MAGIC)))
}

pub(super) static RAMFS_SB_OPS: SuperBlockOps = SuperBlockOps {
    load_inode: ramfs_load_inode,
    evict_inode: ramfs_evict_inode,
    sync_inode: ramfs_sync_inode,
    stat: ramfs_stat,
};
