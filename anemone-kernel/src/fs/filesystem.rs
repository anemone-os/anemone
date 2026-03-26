use core::fmt::Debug;

use crate::prelude::*;

/// VTable a file system type must implement to be mountable.
pub struct FileSystemOps {
    pub name: &'static str,
    pub mount: fn(&MountSource, MountFlags) -> Result<MountedFileSystem, FsError>,
    pub kill_sb: fn(Arc<SuperBlock>),
}

/// A mounted file system, consisting of a superblock and the root inode number.
///
/// Returned by [FileSystem::mount].
pub struct MountedFileSystem {
    pub sb: Arc<SuperBlock>,
    pub root_ino: Ino,
}

/// File system type.
pub struct FileSystem {
    ops: &'static FileSystemOps,
    sb_list: RwLock<Vec<Weak<SuperBlock>>>,
}

impl Debug for FileSystem {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FileSystem")
            .field("name", &self.name())
            .finish()
    }
}

impl FileSystem {
    pub const fn new(ops: &'static FileSystemOps) -> Self {
        Self {
            ops,
            sb_list: RwLock::new(Vec::new()),
        }
    }

    /// Get an existing superblock matching the prediction, or create a new one
    /// with the set function if not found.
    ///
    /// Both the prediction and set functions **must** not try to hold the
    /// `sb_list` lock, or deadlock may occur.
    ///
    /// Intentionally, we do not provide a method called `add_sb` or something
    /// like that. By forcing the caller to provide a prediction function when
    /// adding a new superblock, we can ensure that the caller always checks for
    /// existing superblocks before blindly adding a new one, thus avoiding
    /// potential duplicates and ensuring better consistency in superblock
    /// management.
    pub fn sget<P, S>(&self, prediction: P, set: Option<S>) -> Option<Arc<SuperBlock>>
    where
        P: Fn(&Arc<SuperBlock>) -> bool,
        S: FnOnce() -> Arc<SuperBlock>,
    {
        let mut sb_list = self.sb_list.write_irqsave();

        // first try to find an existing superblock that matches the prediction
        for weak_sb in sb_list.iter() {
            if let Some(sb) = weak_sb.upgrade() {
                if prediction(&sb) {
                    return Some(sb);
                }
            }
        }

        // oops. no existing superblock matches. if we have a set function, create a new
        // one and return it.
        if let Some(set) = set {
            let sb = set();
            sb_list.push(Arc::downgrade(&sb));
            return Some(sb);
        }

        None
    }

    /// Remove superblocks matching the prediction from the list.
    pub fn remove_sb<P>(&self, prediction: P)
    where
        P: Fn(&Arc<SuperBlock>) -> bool,
    {
        let mut sb_list = self.sb_list.write_irqsave();
        sb_list.retain(|weak_sb| {
            if let Some(sb) = weak_sb.upgrade() {
                !prediction(&sb)
            } else {
                // also remove dead weak references
                false
            }
        });
    }
}

// VTable operations reexported here.
impl FileSystem {
    /// Name of the file system, e.g. "btrfs", "xfs", "9p", etc.
    pub fn name(&self) -> &str {
        self.ops.name
    }

    /// Mount a file system from the given source with the given flags.
    ///
    /// The implementation must return a fully initialized [MountedFileSystem]
    /// with a superblock and root inode.
    ///
    /// **NOTE**
    ///
    /// The semantic of this operation depends on the file system type.
    /// For example, a file system backed with some kinds of physical
    /// entity (e.g. block device, network, etc.) may try to find a existing
    /// superblock instance associated with the entity and return it directly
    /// (with [FileSystem::sget]) instead of creating a new one if found,
    /// thus ensuring that all mounts of the same entity share the same
    /// superblock and in-memory inode tree.
    ///
    /// On the other hand, a purely in-memory file system (e.g. ramfs) may
    /// choose to always create a new superblock instance for each mount.
    ///
    /// This function itself does not guarantee any particular semantic; it's up
    /// to the file system implementation to decide how to manage superblocks
    /// and mounts.
    pub fn mount(
        &self,
        source: &MountSource,
        flags: MountFlags,
    ) -> Result<MountedFileSystem, FsError> {
        (self.ops.mount)(source, flags)
    }

    /// Kill a superblock, i.e. clean up all physical resources associated with
    /// the superblock and prepare it for destruction.
    ///
    /// The `sb` is guaranteed to have only one strong reference (the one passed
    /// in), and there is no other reference to the superblock (e.g. exsiting
    /// [InodeRef]s or [Mount]s) when this function is called, so it's safe to
    /// perform cleanup and finalization here.
    pub fn kill_sb(&self, sb: Arc<SuperBlock>) {
        (self.ops.kill_sb)(sb);
    }
}
