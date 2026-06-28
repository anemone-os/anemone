use crate::{
    fs::{inode::Inode, proc::root::PROC_ROOT_INO, superblock::SuperBlockOps},
    prelude::*,
};

fn proc_load_inode(sn: &Arc<SuperBlock>, ino: Ino) -> Result<Arc<Inode>, SysError> {
    unreachable!("proc_load_inode should never be called.")
}

fn proc_evict_inode(inode: Arc<Inode>) -> Result<(), SysError> {
    // no-op. this is just memory release.
    kdebugln!("proc_evict_inode: evicting inode {}", inode.ino());
    Ok(())
}

fn proc_sync_inode(_inode: &InodeRef) -> Result<(), SysError> {
    // no-op
    Ok(())
}

/// Allocate a new inode number.
///
/// Internally, this is only a monotonic counter. But our kenel supports up to
/// 32768 processes, so it should be quite enough. If we ever need to support
/// more processes, we can always switch to a more sophisticated allocator.
pub fn alloc_ino() -> Ino {
    static INO_ALLOC: AtomicU64 = AtomicU64::new(PROC_ROOT_INO.get() + 1);

    // simple CAS loop
    loop {
        let current = INO_ALLOC.load(Ordering::Acquire);
        let next = current
            .checked_add(1)
            .expect("procfs inode number overflow");
        if INO_ALLOC
            .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Ino::new(current);
        }
    }
}

pub static PROC_SB_OPS: SuperBlockOps = SuperBlockOps {
    load_inode: proc_load_inode,
    evict_inode: proc_evict_inode,
    sync_inode: proc_sync_inode,
};
