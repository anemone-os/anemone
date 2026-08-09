use crate::{fs::inode::Inode, prelude::*, utils::any_opaque::NilOpaque};

use super::{
    DEVFS_ROOT_INO, DevfsNodeAttr, DevfsNodeOps, DevfsPublish, inode::devfs_new_inode,
    superblock::DEVFS_SB_OPS,
};

enum DevfsNodeBody {
    Directory(RwLock<DevfsChildren>),
    Leaf(Arc<dyn DevfsNodeOps>),
}

struct DevfsChildren {
    // This ordered collection is the directory-content truth for both lookup
    // and readdir. Keeping one collection avoids a name map and enumeration
    // vector becoming two independently maintained namespace representations.
    ordered: Vec<Arc<DevfsNode>>,
}

impl DevfsChildren {
    const fn new() -> Self {
        Self {
            ordered: Vec::new(),
        }
    }

    fn by_name(&self, name: &str) -> Option<Arc<DevfsNode>> {
        self.ordered.iter().find(|node| node.name == name).cloned()
    }
}

pub(super) struct DevfsNode {
    name: String,
    ino: Ino,
    // Parent identity is immutable and sufficient for `..`. A child must not
    // hold its parent strongly because the parent owns the child namespace.
    parent_ino: Ino,
    attr: DevfsNodeAttr,
    body: DevfsNodeBody,
}

impl DevfsNode {
    fn root() -> Self {
        Self {
            name: String::new(),
            ino: DEVFS_ROOT_INO,
            parent_ino: DEVFS_ROOT_INO,
            attr: DevfsNodeAttr {
                ty: InodeType::Dir,
                perm: InodePerm::all_rwx(),
                rdev: DeviceId::None,
            },
            body: DevfsNodeBody::Directory(RwLock::new(DevfsChildren::new())),
        }
    }

    fn directory(name: String, ino: Ino, parent_ino: Ino) -> Self {
        Self {
            name,
            ino,
            parent_ino,
            attr: DevfsNodeAttr {
                ty: InodeType::Dir,
                perm: InodePerm::all_rwx(),
                rdev: DeviceId::None,
            },
            body: DevfsNodeBody::Directory(RwLock::new(DevfsChildren::new())),
        }
    }

    fn leaf(
        name: String,
        ino: Ino,
        parent_ino: Ino,
        attr: DevfsNodeAttr,
        ops: Arc<dyn DevfsNodeOps>,
    ) -> Self {
        Self {
            name,
            ino,
            parent_ino,
            attr,
            body: DevfsNodeBody::Leaf(ops),
        }
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(super) const fn ino(&self) -> Ino {
        self.ino
    }

    pub(super) const fn parent_ino(&self) -> Ino {
        self.parent_ino
    }

    pub(super) const fn attr(&self) -> DevfsNodeAttr {
        self.attr
    }

    pub(super) fn is_directory(&self) -> bool {
        matches!(&self.body, DevfsNodeBody::Directory(_))
    }

    pub(super) fn leaf_ops(&self) -> Option<&Arc<dyn DevfsNodeOps>> {
        match &self.body {
            DevfsNodeBody::Directory(_) => None,
            DevfsNodeBody::Leaf(ops) => Some(ops),
        }
    }

    pub(super) fn child_by_name(&self, name: &str) -> Option<Arc<DevfsNode>> {
        match &self.body {
            DevfsNodeBody::Directory(children) => children.read().by_name(name),
            DevfsNodeBody::Leaf(_) => None,
        }
    }

    pub(super) fn child_at(&self, index: usize) -> Option<Arc<DevfsNode>> {
        match &self.body {
            DevfsNodeBody::Directory(children) => children.read().ordered.get(index).cloned(),
            DevfsNodeBody::Leaf(_) => None,
        }
    }

    pub(super) fn directory_nlink(&self, inode: &InodeRef) -> Option<u64> {
        match &self.body {
            DevfsNodeBody::Directory(children) => {
                let children = children.read();
                let nlink = 2 + children
                    .ordered
                    .iter()
                    .filter(|child| child.is_directory())
                    .count() as u64;
                // Keep the children snapshot stable while checking the VFS
                // metadata projection. Publication uses the same
                // children->inode-meta lock order.
                assert!(
                    inode.nlink() == nlink,
                    "devfs directory inode link projection diverged from children"
                );
                Some(nlink)
            },
            DevfsNodeBody::Leaf(_) => None,
        }
    }

    fn children(&self) -> &RwLock<DevfsChildren> {
        match &self.body {
            DevfsNodeBody::Directory(children) => children,
            DevfsNodeBody::Leaf(_) => unreachable!("leaf cannot be a publication directory"),
        }
    }
}

enum DevfsChild {
    Directory,
    Leaf {
        attr: DevfsNodeAttr,
        ops: Arc<dyn DevfsNodeOps>,
    },
}

impl DevfsChild {
    fn validate(&self) -> Result<(), SysError> {
        let Self::Leaf { attr, .. } = self else {
            return Ok(());
        };

        // Socket inodes are anonymous VFS objects in the current socket stage.
        // Admit them here only after named socket nodes gain an explicit devfs
        // owner, open contract, and lifecycle.
        if attr.ty == InodeType::Socket {
            return Err(SysError::NotSupported);
        }

        if attr.ty == InodeType::Dir {
            return Err(SysError::InvalidArgument);
        }

        Ok(())
    }
}

/// Opaque capability for publishing direct children into one devfs directory.
#[derive(Clone)]
pub struct DevfsDirectory {
    namespace: Arc<DevfsNamespaceCore>,
    node: Arc<DevfsNode>,
}

impl DevfsDirectory {
    /// Publish one leaf directly below this directory.
    pub fn publish(&self, desc: DevfsPublish) -> Result<Ino, SysError> {
        let child = DevfsChild::Leaf {
            attr: desc.attr,
            ops: desc.ops,
        };
        let node = self.publish_child(desc.name, child)?;
        Ok(node.ino())
    }

    /// Publish one directory directly below this directory.
    pub fn publish_directory(&self, name: String) -> Result<Self, SysError> {
        let node = self.publish_child(name, DevfsChild::Directory)?;
        Ok(Self {
            namespace: self.namespace.clone(),
            node,
        })
    }

    fn publish_child(&self, name: String, child: DevfsChild) -> Result<Arc<DevfsNode>, SysError> {
        if name.is_empty() || name.contains('/') || matches!(name.as_str(), "." | "..") {
            return Err(SysError::InvalidArgument);
        }
        child.validate()?;

        let ino = self.namespace.alloc_ino();
        let node = match child {
            DevfsChild::Directory => Arc::try_new(DevfsNode::directory(name, ino, self.node.ino())),
            DevfsChild::Leaf { attr, ops } => {
                Arc::try_new(DevfsNode::leaf(name, ino, self.node.ino(), attr, ops))
            },
        }
        .map_err(|_| SysError::OutOfMemory)?;
        let inode = devfs_new_inode(self.namespace.sb.clone(), node.clone())?;

        let mut children = self.node.children().write();
        if children.by_name(node.name()).is_some() {
            // Producer capabilities may have non-trivial destructors. Never
            // drop a rejected candidate while holding the namespace lock.
            drop(children);
            drop(inode);
            drop(node);
            return Err(SysError::AlreadyExists);
        }

        // Reserve the only namespace collection before seeding the prepared
        // inode. After the inode enters the persistent icache, the final push
        // cannot allocate and is the sole visibility point.
        if children.ordered.try_reserve(1).is_err() {
            drop(children);
            drop(inode);
            drop(node);
            return Err(SysError::OutOfMemory);
        }

        self.namespace.sb.seed_inode(inode);

        if node.is_directory() {
            let parent_inode = self
                .namespace
                .sb
                .try_iget(self.node.ino())
                .expect("devfs publication parent missing from icache");
            // Inode nlink is a stable VFS metadata projection of the child
            // collection, never a namespace decision source. Both are updated
            // under the parent publication lock and checked by getattr.
            parent_inode.inode().inc_nlink();
        }

        children.ordered.push(node.clone());
        drop(children);

        kdebugln!(
            "devfs: published {} under ino {} with ino {}",
            node.name(),
            self.node.ino(),
            node.ino()
        );

        Ok(node)
    }
}

struct DevfsNamespaceCore {
    sb: Arc<SuperBlock>,
    next_ino: AtomicU64,
}

impl DevfsNamespaceCore {
    fn alloc_ino(&self) -> Ino {
        loop {
            let current = self.next_ino.load(Ordering::Acquire);
            let next = current.checked_add(1).expect("devfs inode number overflow");
            if self
                .next_ino
                .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Ino::new(current);
            }
        }
    }
}

pub(super) struct DevfsNamespace {
    root: DevfsDirectory,
}

impl DevfsNamespace {
    pub(super) fn new(fs: Arc<FileSystem>) -> Result<Self, SysError> {
        let sb = Arc::try_new(SuperBlock::new(
            fs,
            &DEVFS_SB_OPS,
            NilOpaque::new(),
            DEVFS_ROOT_INO,
            MountSource::Pseudo,
        ))
        .map_err(|_| SysError::OutOfMemory)?;
        let root_node = Arc::try_new(DevfsNode::root()).map_err(|_| SysError::OutOfMemory)?;
        let root_inode: Arc<Inode> = devfs_new_inode(sb.clone(), root_node.clone())?;
        sb.seed_inode(root_inode);
        let namespace = Arc::try_new(DevfsNamespaceCore {
            sb,
            next_ino: AtomicU64::new(DEVFS_ROOT_INO.get() + 1),
        })
        .map_err(|_| SysError::OutOfMemory)?;

        Ok(Self {
            root: DevfsDirectory {
                namespace,
                node: root_node,
            },
        })
    }

    pub(super) fn root(&self) -> DevfsDirectory {
        self.root.clone()
    }

    pub(super) fn superblock(&self) -> Arc<SuperBlock> {
        self.root.namespace.sb.clone()
    }
}
