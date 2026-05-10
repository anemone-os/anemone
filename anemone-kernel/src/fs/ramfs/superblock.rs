use crate::{
    fs::{inode::Inode, superblock::SuperBlockOps},
    prelude::*,
};

#[derive(Opaque)]
pub(super) struct RamfsSb {
    next_ino: AtomicU64,
    // transaction lock. used to serialize namespace operations.
    tx_lock: RwLock<()>,
}

impl RamfsSb {
    pub(super) fn new() -> Self {
        Self {
            next_ino: AtomicU64::new(2), // Ino 0 is reserved; root gets ino 1; children start at 2.
            tx_lock: RwLock::new(()),
        }
    }

    pub(super) fn alloc_ino(&self) -> Ino {
        Ino::try_from(self.next_ino.fetch_add(1, Ordering::Relaxed)).unwrap()
    }

    pub(super) fn read_tx<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.tx_lock.read();
        f()
    }

    pub(super) fn write_tx<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.tx_lock.write();
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

pub(super) static RAMFS_SB_OPS: SuperBlockOps = SuperBlockOps {
    load_inode: ramfs_load_inode,
    evict_inode: ramfs_evict_inode,
    sync_inode: ramfs_sync_inode,
};
