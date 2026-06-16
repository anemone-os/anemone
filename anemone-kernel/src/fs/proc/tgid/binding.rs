use crate::{
    fs::proc::{procfs_root_dentries, procfs_sb},
    prelude::*,
};

/// TODO: encapsulate.
#[derive(Debug, Opaque)]
pub struct ThreadGroupBinding {
    pub tg: Arc<ThreadGroup>,
    pub ino: Ino,
    pub alive: AtomicBool,
}

impl ThreadGroupBinding {
    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }
}

/// Invariant:
/// - If a binding exists in this map, then `alive` must be true.
/// - If a binding exists in this map, then its inode must exist in superblock's
///   index.
/// - If a binding disappears from this map, then `alive` must be false, and its
///   inode must be in superblock's ghost index.
static THREAD_GROUP_BINDINGS: Lazy<RwLock<HashMap<Tid, Arc<ThreadGroupBinding>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

/// TODO: make sure no sleeping operations are performed while holding the lock.
static BINDING_TX_LOCK: SpinLock<()> = SpinLock::new(());

/// Called by task topology when a thread group leaves procfs visibility.
///
/// This hook only invalidates procfs binding state. Task topology owns the
/// lifecycle decision and must remove or reap the thread group in the same
/// higher-level transaction so later lookup can only rebuild from active
/// topology.
pub fn invalidate_thread_group_binding(tgid: Tid) {
    let _tx = BINDING_TX_LOCK.lock();

    if let Some(binding) = THREAD_GROUP_BINDINGS.write().remove(&tgid) {
        binding.alive.store(false, Ordering::Release);

        procfs_sb().unindex_inode_by_ino(binding.ino);
        procfs_root_dentries().iter().for_each(|root| {
            match root.remove_child(&tgid.get().to_string()) {
                Ok(()) => {},
                Err(SysError::NotFound) => {
                    // this might happen if /proc/<tgid> is never accessed.
                },
                Err(e) => {
                    kalertln!(
                        "try_unbind_thread_group: failed to remove child for tgid {} from procfs root: {:?}",
                        tgid,
                        e
                    );
                }

            }
        });

        kdebugln!(
            "invalidate_thread_group_binding: invalidated thread group with tgid {}",
            tgid
        );
    } else {
        // this may happen if /proc/<tgid> is never accessed. bindings are
        // lazily created.

        kdebugln!(
            "invalidate_thread_group_binding: no binding found for tgid {}, maybe it was never accessed?",
            tgid
        );
    }
}

/// Called by ordinary wait/reap after a user thread group has already been
/// removed from topology.
pub fn try_unbind_thread_group(tgid: Tid) {
    invalidate_thread_group_binding(tgid);
}

/// [BINDING_TX_LOCK] must be held as first lock if multiple locks should be
/// held.
pub fn binding_tx<F: FnOnce(&mut HashMap<Tid, Arc<ThreadGroupBinding>>) -> R, R>(f: F) -> R {
    let _tx = BINDING_TX_LOCK.lock();

    f(&mut THREAD_GROUP_BINDINGS.write())
}
