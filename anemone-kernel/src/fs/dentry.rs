use core::fmt::Debug;

use crate::prelude::*;

pub struct Dentry {
    name: String,
    parent: Option<Arc<Dentry>>,
    inode: InodeRef,
}

impl Debug for Dentry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Dentry").field("name", &self.name).finish()
    }
}

impl Dentry {
    /// Create a new positive dentry with an inode.
    pub fn new(name: String, parent: Option<&Arc<Dentry>>, inode: InodeRef) -> Self {
        Self {
            name,
            parent: parent.map(Arc::clone),
            inode,
        }
    }

    /// Get the name of this dentry.
    ///
    /// For the root dentry of a mounted filesystem, this will be "/".
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn inode(&self) -> &InodeRef {
        &self.inode
    }

    /// Parent dentry, if any. `None` for the root dentry (i.e. the root of
    /// a mounted filesystem, not the root of the entire namespace).
    pub fn parent(&self) -> Option<Arc<Dentry>> {
        self.parent.as_ref().map(Arc::clone)
    }

    /// Check if this dentry and another dentry refer to the same location in
    /// the namespace.
    ///
    /// Internally, this compares the names and inodes of the two dentries, and
    /// recursively compares their parents.
    pub fn location_eq(&self, other: &Dentry) -> bool {
        if self.name != other.name || self.inode != other.inode {
            return false;
        }

        match (&self.parent, &other.parent) {
            // this is a bit slow...
            (Some(p1), Some(p2)) => p1.location_eq(p2),
            (None, None) => true,
            _ => false,
        }
    }
}
