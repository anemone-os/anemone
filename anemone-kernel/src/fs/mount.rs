use core::fmt::Debug;

use crate::{device::block::BlockDev, prelude::*};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct MountFlags: u32 {
        // The filesystem is mounted read-only. Kernel will enforce this by
        // disallowing any write operations on the mount.
        // const RDONLY = 1 << 0;
    }
}

/// A mount represents a filesystem instance attached somewhere in the
/// namespace. A superblock may be mounted multiple times at different
/// locations.
///
/// Mount is the single source of truth for mounting topology. [Dentry] does not
/// track mount relationships.
pub struct Mount {
    /// Root dentry of this mounted filesystem.
    root: Arc<Dentry>,
    /// The superblock backing this mount.
    sb: Arc<SuperBlock>,
    /// Parent mount.
    ///
    /// For the root mount, this is [None]. For non-root mounts, always [Some].
    parent: Option<Arc<Mount>>,
    /// Child mounts.
    children: SpinLock<Vec<Weak<Mount>>>,
    /// The dentry in the parent mount where this mount is attached.
    ///
    /// For the root mount, this is [None]. For non-root mounts, always [Some].
    mountpoint: Option<Arc<Dentry>>,
    /// Mount flags.
    flags: MountFlags,
}

impl Debug for Mount {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Mount")
            .field("root", &self.root)
            .field("sb", &self.sb)
            .field("flags", &self.flags)
            .finish()
    }
}

#[derive(Debug)]
pub enum MountSource {
    Block(Arc<dyn BlockDev>),
    Pseudo,
}

impl Mount {
    pub fn new(
        root: Arc<Dentry>,
        sb: Arc<SuperBlock>,
        parent: Option<&Arc<Mount>>,
        mountpoint: Option<&Arc<Dentry>>,
        flags: MountFlags,
    ) -> Self {
        Self {
            root,
            sb,
            parent: parent.map(Arc::clone),
            mountpoint: mountpoint.map(Arc::clone),
            flags,
            children: SpinLock::new(Vec::new()),
        }
    }

    pub fn root(&self) -> &Arc<Dentry> {
        &self.root
    }

    pub fn sb(&self) -> &Arc<SuperBlock> {
        &self.sb
    }

    pub fn parent(&self) -> Option<Arc<Mount>> {
        self.parent.as_ref().map(Arc::clone)
    }

    pub fn mountpoint(&self) -> Option<Arc<Dentry>> {
        self.mountpoint.as_ref().map(Arc::clone)
    }

    pub fn flags(&self) -> MountFlags {
        self.flags
    }

    pub fn add_child(&self, child: &Arc<Mount>) {
        self.children.lock().push(Arc::downgrade(child));
    }

    pub fn has_children(&self) -> bool {
        self.children.lock().iter().any(|w| w.upgrade().is_some())
    }

    pub fn remove_child(&self, child: &Arc<Mount>) -> Result<(), FsError> {
        let mut children = self.children.lock();
        let initial_len = children.len();
        children.retain(|weak_child| {
            let Some(strong_child) = weak_child.upgrade() else {
                return false;
            };
            !Arc::ptr_eq(&strong_child, child)
        });
        if children.len() == initial_len {
            Err(FsError::NotFound)
        } else {
            Ok(())
        }
    }

    pub fn child_at(&self, mountpoint: &Arc<Dentry>) -> Option<Arc<Mount>> {
        let mut children = self.children.lock();
        let mut found = None;

        children.retain(|weak_child| {
            let Some(child) = weak_child.upgrade() else {
                return false;
            };

            if found.is_none()
                && child
                    .mountpoint()
                    .as_ref()
                    .is_some_and(|child_mp| child_mp.inode() == mountpoint.inode())
            {
                found = Some(child);
            }

            true
        });

        found
    }
}
