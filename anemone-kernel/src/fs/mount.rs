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

/// Placement of a mount view in a mount tree.
///
/// The authoritative transition owner is `MountTree`; this cached state is kept
/// on `Mount` so old `PathRef`s and diagnostics can still reason about an
/// object after detach. It must not be used to publish topology changes outside
/// a `MountTree` transaction.
#[derive(Clone)]
enum MountPlacement {
    Root,
    Attached {
        parent: Arc<Mount>,
        mountpoint: Arc<Dentry>,
    },
    Detached,
}

impl Debug for MountPlacement {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Root => f.write_str("Root"),
            Self::Attached { parent, mountpoint } => f
                .debug_struct("Attached")
                .field("parent", parent)
                .field("mountpoint", mountpoint)
                .finish(),
            Self::Detached => f.write_str("Detached"),
        }
    }
}

/// A mount represents a filesystem view attached somewhere in the mount tree.
/// A superblock may be mounted multiple times at different locations.
///
/// `MountTree` is the single writer for topology. [Dentry] does not track mount
/// relationships, and ordinary VFS code must not mutate placement through this
/// object directly.
pub struct Mount {
    /// Root dentry of this mounted filesystem.
    root: Arc<Dentry>,
    /// The superblock backing this mount.
    sb: Arc<SuperBlock>,
    /// Current placement published by `MountTree`.
    placement: SpinLock<MountPlacement>,
    /// Direct child mounts in mountpoint-stack order.
    ///
    /// This is a `MountTree`-owned placement cache. Writers must already hold
    /// the tree transaction lock; the spin lock only protects the local vector
    /// while readers take short snapshots.
    children: SpinLock<Vec<Weak<Mount>>>,
    /// Mount flags.
    flags: MountFlags,
}

impl Debug for Mount {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Mount")
            .field("root", &self.root)
            .field("sb", &self.sb)
            .field("placement", &self.placement.lock())
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
    pub fn new(root: Arc<Dentry>, sb: Arc<SuperBlock>, flags: MountFlags) -> Self {
        Self {
            root,
            sb,
            placement: SpinLock::new(MountPlacement::Detached),
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

    fn placement(&self) -> MountPlacement {
        self.placement.lock().clone()
    }

    pub fn parent(&self) -> Option<Arc<Mount>> {
        match self.placement() {
            MountPlacement::Attached { parent, .. } => Some(parent),
            MountPlacement::Root | MountPlacement::Detached => None,
        }
    }

    pub fn mountpoint(&self) -> Option<Arc<Dentry>> {
        match self.placement() {
            MountPlacement::Attached { mountpoint, .. } => Some(mountpoint),
            MountPlacement::Root | MountPlacement::Detached => None,
        }
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

    pub(super) fn mark_root(&self) {
        let mut placement = self.placement.lock();
        assert!(
            matches!(*placement, MountPlacement::Detached),
            "only a detached mount can become root"
        );
        *placement = MountPlacement::Root;
    }

    pub(super) fn mark_attached(&self, parent: &Arc<Mount>, mountpoint: &Arc<Dentry>) {
        let mut placement = self.placement.lock();
        assert!(
            matches!(*placement, MountPlacement::Detached),
            "only a detached mount can be attached"
        );
        *placement = MountPlacement::Attached {
            parent: parent.clone(),
            mountpoint: mountpoint.clone(),
        };
    }

    pub(super) fn mark_detached(&self) {
        let mut placement = self.placement.lock();
        assert!(
            !matches!(*placement, MountPlacement::Root),
            "root mount must not be detached"
        );
        *placement = MountPlacement::Detached;
    }

    pub(super) fn is_reachable(&self) -> bool {
        matches!(
            *self.placement.lock(),
            MountPlacement::Root | MountPlacement::Attached { .. }
        )
    }

    pub(super) fn push_child(&self, child: &Arc<Mount>) {
        self.children.lock().push(Arc::downgrade(child));
    }

    pub(super) fn has_attached_children(&self) -> bool {
        self.children
            .lock()
            .iter()
            .any(|w| w.upgrade().is_some_and(|child| child.is_reachable()))
    }

    pub(super) fn remove_child(&self, child: &Arc<Mount>) -> Result<(), SysError> {
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

    pub(super) fn top_child_at(&self, mountpoint: &Arc<Dentry>) -> Option<Arc<Mount>> {
        let mut children = self.children.lock();

        children.retain(|weak_child| weak_child.upgrade().is_some());

        children.iter().rev().find_map(|weak_child| {
            let child = weak_child
                .upgrade()
                .expect("stale weak child should have been removed above");

            if child.is_reachable()
                && child
                    .mountpoint()
                    .as_ref()
                    .is_some_and(|child_mp| Arc::ptr_eq(child_mp, mountpoint))
            {
                Some(child)
            } else {
                None
            }
        })
    }
}
