use core::fmt::Debug;

use crate::{fs::inode::Inode, prelude::*, utils::any_opaque::AnyOpaque};

/// VTable a superblock must implement.
///
/// Implemented by concrete filesystem types to provide filesystem-specific
/// behavior.
pub(super) struct SuperBlockOps {
    /// Load an inode from the filesystem by its inode number.
    ///
    /// This operation usually involves reading from disk and constructing a
    /// fresh [Inode] instance. The VFS layer inserts the returned inode into
    /// the superblock's resident cache, so subsequent accesses for the same
    /// inode number hit the cache instead of calling this again.
    pub load_inode: fn(&Arc<SuperBlock>, ino: Ino) -> Result<Arc<Inode>, FsError>,

    /// Called when an inode is being evicted from the resident cache.
    ///
    /// Currently we don't have a page cache, so this method is almostly the
    /// same as `sync_inode`.
    ///
    /// This callback runs from an explicit, controlled eviction path — never
    /// from [Drop]. The cache-map lock is **not** held when this runs, so it
    /// is safe to perform blocking I/O (e.g. writeback) here.
    ///
    /// If an [FsError] is returned, the eviction is cancelled and the inode is
    /// re-inserted into the cache. The eviction will be retried later.
    pub evict_inode: fn(&SuperBlock, Arc<Inode>) -> Result<(), FsError>,

    /// Write back the inode to the backing store. This is used to synchronize
    /// metadata updates that may have been made to the inode while it was
    /// resident in the cache.
    ///
    /// **Note that this operation only writes back metadata, not file data.**
    pub sync_inode: fn(&InodeRef) -> Result<(), FsError>,
}

/// A superblock represents a mounted file system instance.
pub struct SuperBlock {
    /// The file system type this superblock belongs to.
    fs: Arc<FileSystem>,
    /// Filesystem-specific operations for this superblock.
    ops: &'static SuperBlockOps,
    /// Private data for the superblock implementation.
    prv: AnyOpaque,
    /// Root inode number of this superblock.
    root_ino: Ino,
    /// Mount source of this superblock.
    backing: MountSource,
    /// Mutable state of superblock.
    inner: RwLock<SuperBlockInner>,
}

impl Debug for SuperBlock {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SuperBlock").field("fs", &self.fs).finish()
    }
}

pub struct SuperBlockInner {
    /// Indexed inodes that are still discoverable by inode number.
    indexed: HashMap<Ino, Arc<Inode>>,
    /// Resident but unindexed inodes. These correspond to deleted/unhashed
    /// objects that are still alive because someone still holds references.
    ghosts: Vec<Arc<Inode>>,
}

impl SuperBlock {
    /// Create a new superblock.
    pub(super) fn new(
        fs: Arc<FileSystem>,
        ops: &'static SuperBlockOps,
        prv: AnyOpaque,
        root_ino: Ino,
        backing: MountSource,
    ) -> Self {
        Self {
            fs,
            ops,
            prv,
            root_ino,
            backing,
            inner: RwLock::new(SuperBlockInner {
                indexed: HashMap::new(),
                ghosts: Vec::new(),
            }),
        }
    }

    /// Which file system type does this superblock belong to.
    pub fn fs(&self) -> &Arc<FileSystem> {
        &self.fs
    }

    /// Get the private data for this superblock, if any.
    pub(super) fn prv(&self) -> &AnyOpaque {
        &self.prv
    }

    /// Get the mount source for this superblock.
    pub fn backing(&self) -> &MountSource {
        &self.backing
    }

    /// Get the root inode of this superblock.
    pub fn root_inode(self: &Arc<Self>) -> InodeRef {
        self.iget(self.root_ino)
            .expect("failed to load root inode for superblock")
    }
}

impl SuperBlock {
    /// Get or load an inode by inode number. This is the canonical way to
    /// obtain inodes from a superblock.
    ///
    /// # Uniqueness invariant
    ///
    /// For a given superblock, two calls to `iget` with the same `ino` while
    /// the inode is resident in the cache will return `Arc::ptr_eq` results.
    ///
    /// # Lock discipline
    ///
    /// This function **must not** be called while holding the superblock inner
    /// lock or any lock that the backend's `load_inode` may itself acquire.
    /// Doing so risks deadlock or re-entrant cache corruption.
    pub(super) fn iget(self: &Arc<Self>, ino: Ino) -> Result<InodeRef, FsError> {
        // fast path
        {
            let inner = self.inner.read_irqsave();
            if let Some(inode) = inner.indexed.get(&ino) {
                return Ok(InodeRef::new(inode.clone()));
            }
        }

        // slow path
        let inode = (self.ops.load_inode)(self, ino)?;

        // re-check and insert. another thread may have loaded concurrently;
        // if so, keep theirs and discard ours.
        {
            let mut inner = self.inner.write_irqsave();
            if let Some(existing) = inner.indexed.get(&ino) {
                return Ok(InodeRef::new(existing.clone()));
            }
            inode.set_indexed(true);
            knoticeln!("superblock: caching inode {:?} loaded from disk", ino);
            inner.indexed.insert(ino, inode.clone());
        }

        Ok(InodeRef::new(inode))
    }

    /// Insert a pre-constructed inode directly into the resident cache.
    ///
    /// This is intended for filesystem `create` paths that build the inode
    /// in-place rather than loading it from a backing store.
    ///
    /// After this operation, the active reference count of the inode will be 1,
    /// for the returned [InodeRef].
    ///
    /// # Panics
    ///
    /// - A live entry for the same [Ino] already exists, as this would violate
    ///   the cache uniqueness invariant.
    /// - The reference count of the provided inode is not zero.
    pub(super) fn seed_inode(&self, inode: Arc<Inode>) -> InodeRef {
        let mut inner = self.inner.write_irqsave();
        let ino = inode.ino();

        #[cfg(debug_assertions)]
        {
            if inode.rc() != 0 {
                panic!(
                    "seed_inode: provided inode has non-zero ref count {:?}",
                    inode.rc()
                );
            }

            if inner.indexed.contains_key(&ino) {
                panic!(
                    "seed_inode: cache already has a live entry for ino {:?}",
                    ino
                );
            }
        }

        inode.set_indexed(true);
        inner.indexed.insert(ino, inode.clone());

        InodeRef::new(inode)
    }

    /// Look up a cached inode by [Ino] without triggering a load.
    ///
    /// Returns [None] if the inode is not resident in the cache.
    /// Use [SuperBlock::iget] for the load-on-miss variant.
    pub(super) fn try_iget(&self, ino: Ino) -> Option<InodeRef> {
        self.inner
            .read_irqsave()
            .indexed
            .get(&ino)
            .cloned()
            .map(InodeRef::new)
    }

    /// Remove an inode from the inode-number index while keeping the object
    /// resident.
    pub(super) fn unindex_inode(&self, inode: &Arc<Inode>) {
        let mut inner = self.inner.write_irqsave();
        let ino = inode.ino();

        if let Some(indexed) = inner.indexed.get(&ino) {
            if Arc::ptr_eq(indexed, inode) {
                inner.indexed.remove(&ino);
            }
        }

        assert!(inode.indexed());
        inode.set_indexed(false);

        // O(n) is a bit slow here. refine this later. we may use generation counts or
        // something similar?
        if !inner.ghosts.iter().any(|ghost| Arc::ptr_eq(ghost, inode)) {
            inner.ghosts.push(inode.clone());
        }
    }

    /// Try to evict an inode by [Ino]. The inode must be resident in the cache
    /// and have zero active references.
    ///
    /// After this operation, the inode will have been removed from the cache.
    ///
    /// Internally, this calls the backend's `evict_inode` callback to allow it
    /// to perform any necessary cleanup or writeback.
    pub(super) fn try_evict(&self, ino: Ino) -> Result<(), FsError> {
        let inode = {
            let inner = self.inner.read_irqsave();
            inner.indexed.get(&ino).cloned().ok_or(FsError::NotFound)?
        };

        self.try_evict_inode(&inode)
    }

    /// Try to evict a specific inode. Note that [SuperBlock::try_evict] only
    /// works when the victim is indexed, but this method can evict both indexed
    /// and ghost inodes.
    pub(super) fn try_evict_inode(&self, inode: &Arc<Inode>) -> Result<(), FsError> {
        if inode.rc() > 0 {
            return Err(FsError::Busy);
        }

        let ino = inode.ino();
        let removed = {
            let mut inner = self.inner.write_irqsave();

            if let Some(indexed) = inner.indexed.get(&ino) {
                if Arc::ptr_eq(indexed, inode) {
                    let removed = inner
                        .indexed
                        .remove(&ino)
                        .expect("indexed inode disappeared");
                    removed.set_indexed(false);
                    Some((removed, true))
                } else {
                    None
                }
            } else if let Some(pos) = inner
                .ghosts
                .iter()
                .position(|ghost| Arc::ptr_eq(ghost, inode))
            {
                Some((inner.ghosts.remove(pos), false))
            } else {
                None
            }
        };

        let (inode, was_indexed) = removed.ok_or(FsError::NotFound)?;
        if let Err(e) = (self.ops.evict_inode)(self, inode.clone()) {
            let mut inner = self.inner.write_irqsave();
            if was_indexed {
                inode.set_indexed(true);
                inner.indexed.insert(ino, inode);
            } else {
                inner.ghosts.push(inode);
            }
            return Err(e);
        }
        knoticeln!("evicted inode {:?} from superblock", ino);
        Ok(())
    }

    /// Check if any inodes in the resident cache have active references, except
    /// for the root inode(s) owned by the mount's root dentry.
    pub(super) fn has_alive_inode(&self) -> bool {
        let inner = self.inner.read_irqsave();
        inner
            .indexed
            .values()
            .chain(inner.ghosts.iter())
            .any(|inode| inode.rc() > 0 && inode.ino() != self.root_ino)
    }

    /// Evict **all** inodes from the resident cache.
    ///
    /// This operation may fail on the first inode that cannot be evicted. In
    /// this case, some inodes may have already been evicted while others
    /// remain. Callers should be prepared to handle this partial eviction
    /// state.
    ///
    /// **This operation will not evict the root inode, since it's pinned by the
    /// mount's root dentry, so it's always referenced and cannot be evicted.**
    pub(super) fn try_evict_all(&self) -> Result<(), FsError> {
        debug_assert!(!self.has_alive_inode());

        let victims: Vec<Arc<Inode>> = {
            let inner = self.inner.read_irqsave();
            inner
                .indexed
                .values()
                .chain(inner.ghosts.iter())
                .filter(|inode| inode.ino() != self.root_ino)
                .cloned()
                .collect()
        };
        for inode in victims {
            self.try_evict_inode(&inode)?;
        }

        Ok(())
    }
}
