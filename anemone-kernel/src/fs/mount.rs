use core::fmt::Debug;

use crate::{device::block::BlockDev, prelude::*};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct MountAttrFlags: u32 {
        // First legacy mount API stage only closes per-mount read-only
        // enforcement. Operation bits such as MS_BIND/MS_MOVE/MS_REMOUNT must
        // never be stored here; they are syscall parser requests.
        const RDONLY = 1 << 0;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct MountFlags: u32 {
        // The filesystem is mounted read-only. Kernel will enforce this by
        // disallowing any write operations on the mount.
        const RDONLY = 1 << 0;
    }
}

impl From<MountAttrFlags> for MountFlags {
    fn from(value: MountAttrFlags) -> Self {
        let mut flags = Self::empty();
        if value.contains(MountAttrFlags::RDONLY) {
            flags |= Self::RDONLY;
        }
        flags
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountData {
    Null,
    Text(Box<str>),
}

impl MountData {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Null => true,
            Self::Text(data) => data.is_empty(),
        }
    }

    pub fn has_loop_option(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Text(data) => data
                .split(',')
                .map(str::trim)
                .any(|option| option == "loop" || option.starts_with("loop=")),
        }
    }

    pub fn reject_nonempty_for(&self, fs_name: &str) -> Result<(), SysError> {
        if self.is_empty() {
            return Ok(());
        }

        knoticeln!(
            "mount: filesystem {} rejects non-empty legacy data: empty=false contains_loop={}",
            fs_name,
            self.has_loop_option()
        );
        Err(SysError::InvalidArgument)
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn test_mount_data_loop_option_detection_trims_options() {
        assert!(MountData::Text(Box::from("rw, loop")).has_loop_option());
        assert!(MountData::Text(Box::from("loop=/tmp/disk.img")).has_loop_option());
        assert!(!MountData::Text(Box::from("rw")).has_loop_option());
        assert!(!MountData::Null.has_loop_option());
    }

    #[kunit]
    fn test_mount_data_reject_nonempty_for_backend() {
        assert!(MountData::Null.reject_nonempty_for("kunit").is_ok());
        assert_eq!(
            MountData::Text(Box::from("size=64m"))
                .reject_nonempty_for("kunit")
                .unwrap_err(),
            SysError::InvalidArgument
        );
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

    pub fn ensure_writable(&self) -> Result<(), SysError> {
        if self.flags.contains(MountFlags::RDONLY) {
            Err(SysError::ReadOnlyFs)
        } else {
            Ok(())
        }
    }

    pub fn add_child(&self, child: &Arc<Mount>) {
        self.children.lock().push(Arc::downgrade(child));
    }

    pub fn has_children(&self) -> bool {
        self.children.lock().iter().any(|w| w.upgrade().is_some())
    }

    pub fn remove_child(&self, child: &Arc<Mount>) -> Result<(), SysError> {
        let mut children = self.children.lock();
        let initial_len = children.len();
        children.retain(|weak_child| {
            let Some(strong_child) = weak_child.upgrade() else {
                return false;
            };
            !Arc::ptr_eq(&strong_child, child)
        });
        if children.len() == initial_len {
            Err(SysError::NotFound)
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
                    .is_some_and(|child_mp| Arc::ptr_eq(child_mp, mountpoint))
            {
                found = Some(child);
            }

            true
        });

        found
    }
}
