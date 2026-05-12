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

/// Called by task management code when a thread group is reaped.
///
/// Order matters: unbind thread group first, then unindex inode.
pub fn try_unbind_thread_group(tgid: Tid) {
    let _tx = BINDING_TX_LOCK.lock();

    if let Some(binding) = THREAD_GROUP_BINDINGS.write().remove(&tgid) {
        binding.alive.store(false, Ordering::Release);

        // we use map here since procfs might not be mounted.

        procfs_sb().map(|sb| sb.unindex_inode_by_ino(binding.ino));
        procfs_root_dentries().map(|roots| {
            for root in roots {
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
            }
        });

        kdebugln!(
            "try_unbind_thread_group: unbound thread group with tgid {}",
            tgid
        );
    } else {
        // this may happen if /proc/<tgid> is never accessed. bindings are
        // lazily created.

        kdebugln!(
            "try_unbind_thread_group: no binding found for tgid {}, maybe it was never accessed?",
            tgid
        );
    }
}

/// [BINDING_TX_LOCK] must be held as first lock if multiple locks should be
/// held.
pub fn binding_tx<F: FnOnce(&mut HashMap<Tid, Arc<ThreadGroupBinding>>) -> R, R>(f: F) -> R {
    let _tx = BINDING_TX_LOCK.lock();

    f(&mut THREAD_GROUP_BINDINGS.write())
}
