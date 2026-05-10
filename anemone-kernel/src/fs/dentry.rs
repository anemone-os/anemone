use core::fmt::Debug;

use crate::prelude::*;

/// A [Dentry] determines the location of a file in a mounted filesystem.
///
/// See [PathRef] for absolute location in the entire namespace.
pub struct Dentry {
    parent: Option<Arc<Dentry>>,
    inode: InodeRef,
    inner: RwLock<DentryInner>,
}

struct DentryInner {
    name: String,
    /// For directories this is [Some], otherwise [None].
    children: Option<HashMap<String, Weak<Dentry>>>,
}

impl Debug for Dentry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Dentry")
            .field("name", &self.name())
            .finish()
    }
}

impl Dentry {
    /// Create a new dentry with an inode.
    ///
    /// If the inode represents a directory, `children` will be initialized as
    /// an empty map, otherwise [None].
    pub fn new(name: String, parent: Option<Arc<Dentry>>, inode: InodeRef) -> Self {
        Self {
            parent,
            inner: RwLock::new(DentryInner {
                name,
                children: if inode.ty() == InodeType::Dir {
                    Some(HashMap::new())
                } else {
                    None
                },
            }),
            inode,
        }
    }

    /// Get the name of this dentry.
    ///
    /// For the root dentry of a mounted filesystem, this will be "/".
    pub fn name(&self) -> String {
        self.inner.read().name.clone()
    }

    /// Get the inode of this dentry.
    pub fn inode(&self) -> &InodeRef {
        &self.inode
    }

    /// Parent dentry, if any. `None` for the root dentry (i.e. the root of
    /// a mounted filesystem, not the root of the entire namespace).
    pub fn parent(&self) -> Option<Arc<Dentry>> {
        self.parent.as_ref().map(Arc::clone)
    }

    /// Try to insert a child dentry with the given name.
    pub fn insert_child(&self, name: String, dentry: &Arc<Dentry>) -> Result<(), SysError> {
        if let Some(children) = self.inner.write().children.as_mut() {
            if let Some(record) = children.get(&name) {
                if record.upgrade().is_some() {
                    return Err(SysError::AlreadyExists);
                } else {
                    children.remove(&name);
                }
            }
            children.insert(name, Arc::downgrade(dentry));
            Ok(())
        } else {
            Err(SysError::NotDir)
        }
    }

    /// Remove a child dentry with the given name.
    pub fn remove_child(&self, name: &str) -> Result<(), SysError> {
        if let Some(children) = self.inner.write().children.as_mut() {
            if let Some(existing) = children.get(name).and_then(|weak| weak.upgrade()) {
                children.remove(name);
                Ok(())
            } else {
                // though the weak reference exists, it's not counted as a child if it can't be
                // upgraded.
                // TODO: remove the stale weak reference.
                Err(SysError::NotFound)
            }
        } else {
            Err(SysError::NotDir)
        }
    }

    /// Look up a child dentry with the given name.
    pub fn lookup_child(&self, name: &str) -> Result<Arc<Dentry>, SysError> {
        if let Some(children) = self.inner.write().children.as_mut() {
            if let Some(record) = children.get(name) {
                if let Some(child) = record.upgrade() {
                    Ok(child)
                } else {
                    children.remove(name);
                    Err(SysError::NotFound)
                }
            } else {
                Err(SysError::NotFound)
            }
        } else {
            Err(SysError::NotDir)
        }
    }
}
