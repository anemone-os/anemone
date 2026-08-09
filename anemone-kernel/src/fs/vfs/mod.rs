// We prefer gathering all public APIs in this module, and keep the global state
// hidden in a singleton struct, which helps a lot to ensure lock ordering.
use super::mount::MountTree;
use crate::prelude::*;

/// Virtual file system. Singleton instance.
///
/// **LOCK ORDERING:**
/// **`visible` -> `anonymous` -> `fs_list` → `mounts` → `root_mount`**
struct VfsSubSys {
    /// Global mount tree. Path resolution occurs here. For those
    /// filesystems that should be exposed to user space. e.g. disk-backed
    /// filesystems, devfs, sysfs, etc.
    visible: MountTree,
    /// Anonymous mount tree. For those kernel-internal pseudo file systems.
    /// e.g. pipefs, sockfs, etc.
    anonymous: MountTree,
    fs_list: RwLock<Vec<Arc<FileSystem>>>,
}

static VFS: Lazy<VfsSubSys> = Lazy::new(|| VfsSubSys {
    visible: MountTree::new(),
    anonymous: MountTree::new(),
    fs_list: RwLock::new(Vec::new()),
});

/// Register a file system type.
///
/// On success, returns an `Arc` to the registered `FileSystem`.
pub fn register_filesystem(fs: &'static FileSystemOps) -> Result<Arc<FileSystem>, SysError> {
    let mut fs_list = VFS.fs_list.write();
    for existing in fs_list.iter() {
        if existing.name() == fs.name {
            return Err(SysError::AlreadyExists);
        }
    }
    kinfoln!("registered filesystem: {}", fs.name);
    let fs = Arc::new(FileSystem::new(fs));
    fs_list.push(fs.clone());

    Ok(fs)
}

/// Retrieve a file system type by name.
pub fn get_filesystem(name: &str) -> Option<Arc<FileSystem>> {
    let fs_list = VFS.fs_list.read();
    for fs in fs_list.iter() {
        if fs.name() == name {
            return Some(fs.clone());
        }
    }
    None
}

/// Mount a filesystem into visible namespace.
///
/// If no root mount exists yet, the new mount becomes the root mount.
pub fn mount_at(
    fs_name: &str,
    source: MountSource,
    attrs: MountAttrFlags,
    mountpoint: &PathRef,
) -> Result<Arc<Mount>, SysError> {
    let fs = get_filesystem(fs_name).ok_or(SysError::NotFound)?;

    VFS.visible.mount_at(fs, source, attrs, mountpoint)
}

/// Mount a filesystem into visible namespace with legacy mount data.
///
/// Only syscall adapters should call this entry. Internal callers use
/// `mount_at` so they cannot accidentally propagate legacy user ABI data.
pub fn mount_at_with_data(
    fs_name: &str,
    source: MountSource,
    attrs: MountAttrFlags,
    data: MountData,
    mountpoint: &PathRef,
) -> Result<Arc<Mount>, SysError> {
    let fs = get_filesystem(fs_name).ok_or(SysError::NotFound)?;

    VFS.visible
        .mount_at_with_data(fs, source, attrs, data, mountpoint)
}

/// Mount a filesystem into visible namespace as the root mount.
pub fn mount_root(
    fs_name: &str,
    source: MountSource,
    attrs: MountAttrFlags,
) -> Result<Arc<Mount>, SysError> {
    let fs = get_filesystem(fs_name).ok_or(SysError::NotFound)?;

    VFS.visible.mount_root(fs, source, attrs)
}

/// Update per-mount attributes for the currently visible mount view.
pub fn remount_attrs(target: &PathRef, attrs: MountAttrFlags) -> Result<(), SysError> {
    VFS.visible.remount_attrs(target, attrs)
}

/// Create a bind mount view inside the visible mount tree.
pub fn bind_mount(source: &PathRef, target: &PathRef, recursive: bool) -> Result<usize, SysError> {
    VFS.visible.bind_mount(source, target, recursive)
}

/// Move an attached mount view inside the visible mount tree.
pub fn move_mount(source: &PathRef, target: &PathRef) -> Result<usize, SysError> {
    VFS.visible.move_mount(source, target)
}

/// Accept private propagation requests for the currently private tree.
pub fn make_mount_private(target: &PathRef, recursive: bool) -> Result<usize, SysError> {
    VFS.visible.make_private(target, recursive)
}

/// **Called by anonymous filesystem driver. DO NOT TOUCH THIS.**
pub(in crate::fs) fn mount_early_anonymous_root(
    anony_fs: Arc<FileSystem>,
) -> Result<Arc<Mount>, SysError> {
    VFS.anonymous.mount_early_pseudo_root(anony_fs)
}

/// Unmount a filesystem from visible namespace.
pub fn unmount(mount: Arc<Mount>) -> Result<(), SysError> {
    VFS.visible.unmount(&mount)
}

/// Lazily detach a filesystem subtree from visible namespace.
pub fn lazy_unmount(mount: Arc<Mount>) -> Result<usize, SysError> {
    VFS.visible.lazy_unmount(&mount)
}

/// Snapshot visible mount views in mount-tree attach order.
pub fn visible_mounts_snapshot() -> Vec<Arc<Mount>> {
    VFS.visible.mounts()
}

/// Get the root [PathRef] of the visible namespace.
///
/// # Panics
///
/// Panics if the root mount has not been established yet. This should never
/// happen after the initial filesystem has been mounted during boot.
pub fn root_pathref() -> PathRef {
    VFS.visible
        .root_path()
        .expect("root mount must be established")
}

/// Get the root [PathRef] of the anonymous namespace.
pub fn anonymous_root_pathref() -> PathRef {
    VFS.anonymous
        .root_path()
        .expect("anonymous root mount must be established")
}

/// For visible mount tree.
fn mounted_superblocks_for(tree: &MountTree) -> Vec<Arc<SuperBlock>> {
    let mounts = tree.mounts();
    let mut superblocks = Vec::new();

    for mount in mounts.iter() {
        let sb = mount.sb().clone();
        if superblocks
            .iter()
            .any(|existing| Arc::ptr_eq(existing, &sb))
        {
            continue;
        }
        superblocks.push(sb);
    }

    superblocks
}

pub fn mounted_superblocks() -> Vec<Arc<SuperBlock>> {
    mounted_superblocks_for(&VFS.visible)
}

/// Called when the system is shutting down. This makes one best-effort
/// resident snapshot per mounted superblock, writes it back, and then asks
/// each filesystem to commit its filesystem-wide state.
pub unsafe fn on_shutdown() {
    let mut superblocks = mounted_superblocks_for(&VFS.anonymous);
    for sb in mounted_superblocks_for(&VFS.visible) {
        if !superblocks
            .iter()
            .any(|existing| Arc::ptr_eq(existing, &sb))
        {
            superblocks.push(sb);
        }
    }

    for sb in superblocks {
        sb.sync_resident_inodes_best_effort();
        if let Err(err) = sb.fs().sync_fs(&sb) {
            kerrln!(
                "failed to sync filesystem {} during shutdown: {:?}",
                sb.fs().name(),
                err
            );
        }
    }
}

pub fn mount_stack_top_at(parent: &Arc<Mount>, mountpoint: &Arc<Dentry>) -> Option<Arc<Mount>> {
    VFS.visible
        .top_child_at(parent, mountpoint)
        .or_else(|| VFS.anonymous.top_child_at(parent, mountpoint))
}

pub fn mount_placement_generation() -> (u64, u64) {
    (
        VFS.visible.placement_generation(),
        VFS.anonymous.placement_generation(),
    )
}

mod open;
mod ops;

pub(crate) use open::vfs_open_description;
pub use ops::*;
