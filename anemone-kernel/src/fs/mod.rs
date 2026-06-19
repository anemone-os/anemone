//! Virtual file system and filesystem drivers.

// vfs infrastructure
mod anonymous;
mod cache_stats;
mod dentry;
mod eventfd;
pub mod fanotify;
// mod error;
mod file;
mod filesystem;
mod inode;
mod inode_shrinker;
mod iomux;
mod mount;
mod namei;
mod path;
mod permission;
mod superblock;
pub mod timerfd;

// filesystem drivers
pub mod devfs;
#[cfg(feature = "fs_ext4")]
mod ext4;
mod pipe;

pub mod proc;

mod ramfs;

pub mod api;

pub use self::{
    anonymous::*,
    dentry::Dentry,
    file::{
        BackingFileHandle, DirEntry, DirSink, FcntlAccess, FcntlCtx, File, FileFcntlCmd,
        FileFcntlHook, FileFcntlOutcome, FileIoCtx, FileMode, FileOpStatusFlags, FileOps,
        FixedSizeDirSink, IoctlArgFdLookup, IoctlArgFile, IoctlCtx, IoctlFileAccess, ReadDirResult,
        SeekFrom, SinkResult, accept_file_op_status_flags, seek_dir_rewind, seek_with_bounded_size,
        seek_with_fixed_size, seek_with_inode_size,
    },
    filesystem::{FileSystem, FileSystemFlags, FileSystemOps},
    inode::{
        DeviceId, Ino, InoIsZero, InodeMeta, InodeMode, InodeOps, InodePerm, InodeRef, InodeStat,
        InodeType, ModifType, OpenedFile,
    },
    iomux::{PollEvent, PollRegisterResult, PollRequest},
    mount::{Mount, MountAttrFlags, MountData, MountSource},
    namei::{
        ResolveFlags, resolve, resolve_from, resolve_from_with_root,
        resolve_from_with_root_checked, resolve_parent, resolve_parent_from,
        resolve_parent_from_with_root, resolve_parent_from_with_root_checked,
    },
    path::PathRef,
    permission::{FsAccess, FsPermChecker},
    superblock::SuperBlock,
};
pub use cache_stats::resident_file_inode_cache_pages;
pub use inode_shrinker::init_inode_shrinker;

// We prefer gathering all public APIs in this module, and keep the global state
// hidden in a singleton struct, which helps a lot to ensure lock ordering.
mod vfs {
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
    pub fn bind_mount(
        source: &PathRef,
        target: &PathRef,
        recursive: bool,
    ) -> Result<usize, SysError> {
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

    pub fn resident_file_cache_pages() -> usize {
        let mut superblocks = mounted_superblocks_for(&VFS.anonymous);
        superblocks.extend(mounted_superblocks_for(&VFS.visible));

        let mut unique = Vec::<Arc<SuperBlock>>::new();
        for sb in superblocks {
            if unique.iter().any(|existing| Arc::ptr_eq(existing, &sb)) {
                continue;
            }
            unique.push(sb);
        }

        unique
            .iter()
            .map(|sb| sb.resident_file_cache_pages())
            .sum()
    }

    /// Called when the system is shutting down. This will flush all cached data
    /// to storage devices of file systems, if exist, and perform any necessary
    /// cleanup.
    pub unsafe fn on_shutdown() {
        fn sync_superblocks(tree: &MountTree) {
            for sb in mounted_superblocks_for(tree) {
                if let Err(err) = sb.fs().sync_fs(&sb) {
                    kerrln!(
                        "failed to sync filesystem {} during shutdown: {:?}",
                        sb.fs().name(),
                        err
                    );
                }
            }
        }

        sync_superblocks(&VFS.anonymous);
        sync_superblocks(&VFS.visible);
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
}
pub use vfs::*;

/// POD struct representing a path resolution request.
#[derive(Debug, Clone, Copy)]
pub struct PathResolution<'a> {
    pub target: &'a crate::prelude::Path,
    pub flags: ResolveFlags,
}

impl<'a, 'p, P> From<&'p P> for PathResolution<'a>
where
    P: AsRef<crate::prelude::Path> + 'p,
    'p: 'a,
{
    fn from(path: &'p P) -> Self {
        Self::normal(path.as_ref())
    }
}

impl<'a> From<&'a crate::prelude::Path> for PathResolution<'a> {
    fn from(path: &'a crate::prelude::Path) -> Self {
        Self::normal(path)
    }
}

impl<'a> PathResolution<'a> {
    /// Create a `PathResolution` with the given path and default flags.
    ///
    /// `default` here means no flags are set, i.e. the resolution will follow
    /// all symlinks.
    pub fn normal(target: &'a crate::prelude::Path) -> Self {
        Self {
            target,
            flags: ResolveFlags::empty(),
        }
    }

    pub fn new(target: &'a crate::prelude::Path, flags: ResolveFlags) -> Self {
        Self { target, flags }
    }
}

/// These operations target the global filesystem state.
///
/// Comsumers are always kernel threads. User threads use [Task::lookup_path]
/// and so on to access the filesystem.
mod vfs_ops {
    use crate::{
        fs::{
            mount_stack_top_at,
            namei::{materialize_child_dentry, resolve, resolve_parent},
            unmount,
        },
        prelude::*,
    };

    mod primitives {
        use crate::fs::{inode::RenameFlags, namei::resolve_parent_from};

        use super::*;

        fn new_inode_perm(parent: &InodeRef, ty: InodeType, mut perm: InodePerm) -> InodePerm {
            let checker = FsPermChecker::for_current_fs();
            let parent_perm = parent.inode().perm();

            if ty == InodeType::Dir {
                perm.remove(InodePerm::ISUID | InodePerm::ISGID);
                if parent_perm.contains(InodePerm::ISGID) {
                    perm.insert(InodePerm::ISGID);
                }
                return perm;
            }

            if perm.contains(InodePerm::ISGID)
                && perm.contains(InodePerm::IXGRP)
                && parent_perm.contains(InodePerm::ISGID)
                && !checker.fs_group_allowed(parent.gid())
                && !checker.has_cap(Capability::FSETID)
            {
                perm.remove(InodePerm::ISGID);
            }
            perm
        }

        fn init_new_inode_owner(parent: &InodeRef, inode: &InodeRef, perm: InodePerm) {
            let cred = get_current_task().cred();
            let group = if parent.inode().perm().contains(InodePerm::ISGID) {
                parent.gid()
            } else {
                cred.gid.fs
            };
            let ctime = Instant::now().to_duration();

            inode.chown(Some(cred.uid.fs), Some(group), ctime);
            inode.chmod(perm, ctime);
        }

        /// Mount a filesystem at the specified mountpoint.
        pub fn vfs_mount_at<'a, R: Into<PathResolution<'a>>>(
            fs_name: &str,
            source: MountSource,
            attrs: MountAttrFlags,
            mountpoint: R,
        ) -> Result<Arc<Mount>, SysError> {
            let mountpoint = mountpoint.into();
            let mountpoint = resolve(mountpoint.target, mountpoint.flags)?;

            if mountpoint.inode().ty() != InodeType::Dir {
                return Err(SysError::NotDir);
            }

            mount_at(fs_name, source, attrs, &mountpoint)
        }

        /// Unmount a filesystem at the specified mountpoint.
        pub fn vfs_unmount<'a, R: Into<PathResolution<'a>>>(mountpoint: R) -> Result<(), SysError> {
            let mountpoint = mountpoint.into();
            let mountpoint = resolve(mountpoint.target, mountpoint.flags)?;
            // The path must point at the root of a mounted filesystem, not an
            // arbitrary entry inside one.
            let mount_root = mountpoint.mount().root();
            if !Arc::ptr_eq(mountpoint.dentry(), &mount_root) {
                return Err(SysError::NotMounted);
            }
            unmount(mountpoint.mount().clone())
        }

        /// Look up a path and return a [`PathRef`] to the target.
        ///
        /// Internally, this is simply a thin wrapper around
        /// [fs::namei::resolve].
        pub fn vfs_lookup<'a, R: Into<PathResolution<'a>>>(path: R) -> Result<PathRef, SysError> {
            let path = path.into();
            resolve(path.target, path.flags)
        }

        /// Look up a path relative to a directory and return a [`PathRef`] to
        /// the target.
        ///
        /// Internally, this is simply a thin wrapper around
        /// [fs::namei::resolve_from].
        pub fn vfs_lookup_from<'a, R: Into<PathResolution<'a>>>(
            dir: &PathRef,
            rel_path: R,
        ) -> Result<PathRef, SysError> {
            let rel_path = rel_path.into();
            resolve_from(dir, rel_path.target, rel_path.flags)
        }

        pub fn vfs_touch<'a, R: Into<PathResolution<'a>>>(
            path: R,
            perm: InodePerm,
        ) -> Result<PathRef, SysError> {
            vfs_touch_at(&root_pathref(), path.into(), perm)
        }

        pub fn vfs_touch_at<'a, R: Into<PathResolution<'a>>>(
            dir: &PathRef,
            rel_path: R,
            perm: InodePerm,
        ) -> Result<PathRef, SysError> {
            let rel_path = rel_path.into();
            let (parent, name) = resolve_parent_from(dir, rel_path.target, rel_path.flags)?;

            parent.mount().ensure_writable()?;

            let perm = new_inode_perm(parent.inode(), InodeType::Regular, perm);
            let inode = parent.inode().touch(&name, perm)?;
            init_new_inode_owner(parent.inode(), &inode, perm);

            let dentry = materialize_child_dentry(parent.dentry(), &name, inode)?;

            Ok(PathRef::new(parent.mount().clone(), dentry))
        }

        pub fn vfs_open<'a, R: Into<PathResolution<'a>>>(path: R) -> Result<File, SysError> {
            vfs_open_at(&root_pathref(), path)
        }

        pub fn vfs_open_at<'a, R: Into<PathResolution<'a>>>(
            dir: &PathRef,
            rel_path: R,
        ) -> Result<File, SysError> {
            let rel_path = rel_path.into();
            let pathref = resolve_from(dir, rel_path.target, rel_path.flags)?;
            pathref.open()
        }

        pub fn vfs_get_attr<'a, R: Into<PathResolution<'a>>>(
            path: R,
        ) -> Result<InodeStat, SysError> {
            let path = path.into();
            resolve(path.target, path.flags)?.inode().get_attr()
        }

        pub fn vfs_mkdir<'a, R: Into<PathResolution<'a>>>(
            path: R,
            perm: InodePerm,
        ) -> Result<PathRef, SysError> {
            vfs_mkdir_at(&root_pathref(), path.into(), perm)
        }

        pub fn vfs_mkdir_at<'a, R: Into<PathResolution<'a>>>(
            dir: &PathRef,
            rel_path: R,
            perm: InodePerm,
        ) -> Result<PathRef, SysError> {
            let rel_path = rel_path.into();
            let (parent, name) = resolve_parent_from(dir, rel_path.target, rel_path.flags)?;

            parent.mount().ensure_writable()?;

            let perm = new_inode_perm(parent.inode(), InodeType::Dir, perm);
            let inode = parent.inode().mkdir(&name, perm)?;
            init_new_inode_owner(parent.inode(), &inode, perm);

            let dentry = materialize_child_dentry(parent.dentry(), &name, inode)?;

            Ok(PathRef::new(parent.mount().clone(), dentry))
        }

        /// Hard link of symlinks is not allowed. So we use [Path] instead of
        /// [PathResolution] for both, to avoid confusion.
        pub fn vfs_link(old_path: &Path, new_path: &Path) -> Result<(), SysError> {
            let target = resolve(old_path, ResolveFlags::empty())?;
            if target.inode().ty() == InodeType::Dir {
                return Err(SysError::IsDir);
            }

            let (parent, name) = resolve_parent(new_path, ResolveFlags::empty())?;
            vfs_link_at(&target, &parent, &name)
        }

        pub fn vfs_link_at(
            target: &PathRef,
            new_parent: &PathRef,
            new_name: &str,
        ) -> Result<(), SysError> {
            if new_name.is_empty() || new_name.contains('/') || matches!(new_name, "." | "..") {
                return Err(SysError::InvalidArgument);
            }

            if target.inode().ty() == InodeType::Dir {
                return Err(SysError::IsDir);
            }

            new_parent.mount().ensure_writable()?;
            new_parent.inode().link(new_name, target.inode())?;

            Ok(())
        }

        /// Create a symbolic link at `link_path` pointing to `target`.
        pub fn vfs_symlink<'a, R: Into<PathResolution<'a>>>(
            target: &Path,
            link_path: R,
        ) -> Result<PathRef, SysError> {
            vfs_symlink_at(&root_pathref(), target, link_path)
        }

        pub fn vfs_symlink_at<'a, R: Into<PathResolution<'a>>>(
            dir: &PathRef,
            target: &Path,
            rel_path: R,
        ) -> Result<PathRef, SysError> {
            let rel_path = rel_path.into();
            if target.components().next().is_none() {
                // empty symlink is not allowed.
                return Err(SysError::InvalidArgument);
            }

            let (parent, name) = resolve_parent_from(dir, rel_path.target, rel_path.flags)?;
            parent.mount().ensure_writable()?;
            let inode = parent.inode().symlink(&name, target)?;
            init_new_inode_owner(parent.inode(), &inode, InodePerm::all_rwx());
            let dentry = materialize_child_dentry(parent.dentry(), &name, inode)?;

            Ok(PathRef::new(parent.mount().clone(), dentry))
        }

        /// See [vfs_link] for the reason why we use [Path] instead of
        /// [PathResolution] here.
        pub fn vfs_unlink(path: &Path) -> Result<(), SysError> {
            vfs_unlink_at(&root_pathref(), path)
        }

        /// See [vfs_link] for the reason why we use [Path] instead of
        /// [PathResolution] here.
        pub fn vfs_unlink_at(dir: &PathRef, rel_path: &Path) -> Result<(), SysError> {
            let (parent, name) = resolve_parent_from(dir, rel_path, ResolveFlags::empty())?;
            parent.mount().ensure_writable()?;
            parent.inode().unlink(&name)?;

            // remove the dentry from the cache to prevent stale lookups. the child
            // may never have been cached, which is not an error.
            match parent.dentry().remove_child(&name) {
                Ok(()) | Err(SysError::NotFound) => (),
                Err(err) => return Err(err),
            }

            Ok(())
        }

        /// By POSIX convention, rename won't follow last symlink. instead, it
        /// rename the symlink itself. So [PathResolution] is not used here.
        ///
        /// TODO: refine.
        pub fn vfs_rename_at(
            old_path: &PathRef,
            new_dir: &PathRef,
            new_name: &str,
            flags: RenameFlags,
        ) -> Result<(), SysError> {
            // dentry modification must be done here to avoid stale dentries.

            flags.validate()?;

            if new_name.is_empty() || new_name.contains('/') || matches!(new_name, "." | "..") {
                return Err(SysError::InvalidArgument);
            }

            if new_dir.inode().ty() != InodeType::Dir {
                return Err(SysError::NotDir);
            }

            if !Arc::ptr_eq(old_path.mount(), new_dir.mount()) {
                return Err(SysError::CrossDeviceLink);
            }

            let Some(old_parent) = old_path.dentry().parent() else {
                return Err(SysError::Busy);
            };

            let old_name = old_path.dentry().name();

            if old_name == new_name && Arc::ptr_eq(&old_parent, new_dir.dentry()) {
                return Ok(());
            }

            old_path.mount().ensure_writable()?;

            if let Ok(existing) = new_dir.dentry().lookup_child(new_name) {
                if mount_stack_top_at(new_dir.mount(), &existing).is_some() {
                    return Err(SysError::Busy);
                }
            }

            if old_path.inode().ty() == InodeType::Dir {
                let mut cur = Some(new_dir.dentry().clone());
                while let Some(dentry) = cur {
                    if Arc::ptr_eq(&dentry, old_path.dentry()) {
                        return Err(SysError::InvalidArgument);
                    }
                    cur = dentry.parent();
                }
            }

            old_parent
                .inode()
                .rename(&old_name, new_dir.inode(), new_name, flags)?;

            match old_parent.remove_child(&old_name) {
                Ok(()) | Err(SysError::NotFound) => (),
                Err(err) => return Err(err),
            }

            match new_dir.dentry().remove_child(new_name) {
                Ok(()) | Err(SysError::NotFound) => (),
                Err(err) => return Err(err),
            }

            Ok(())
        }

        /// Read the target of a symbolic link.
        pub fn vfs_read_link(path: &Path) -> Result<PathBuf, SysError> {
            vfs_read_link_at(&root_pathref(), path)
        }

        /// Read the target of a symbolic link.
        pub fn vfs_read_link_at(dir: &PathRef, rel_path: &Path) -> Result<PathBuf, SysError> {
            let pathref = resolve_from(dir, rel_path, ResolveFlags::UNFOLLOW_LAST_SYMLINK)?;
            let inode = pathref.inode();
            if inode.ty() != InodeType::Symlink {
                return Err(SysError::NotSymlink);
            }
            inode.read_link()
        }

        pub fn vfs_rmdir_at<'a, R: Into<PathResolution<'a>>>(
            dir: &PathRef,
            rel_path: R,
        ) -> Result<(), SysError> {
            let rel_path = rel_path.into();
            let target = resolve_from(dir, rel_path.target, rel_path.flags)?;
            if target.inode().ty() != InodeType::Dir {
                return Err(SysError::NotDir);
            }

            let (parent, name) = resolve_parent_from(
                dir,
                rel_path.target,
                rel_path.flags.remove_last_symlink_flags(),
            )?;

            if !Arc::ptr_eq(target.mount(), parent.mount()) {
                return Err(SysError::IsMountPoint);
            }

            parent.mount().ensure_writable()?;

            parent.inode().rmdir(&name)?;

            // remove the dentry from the cache to prevent stale lookups. the child
            // may never have been cached, which is not an error.
            match parent.dentry().remove_child(&name) {
                Ok(()) | Err(SysError::NotFound) => (),
                Err(err) => return Err(err),
            }

            Ok(())
        }

        pub fn vfs_rmdir<'a, R: Into<PathResolution<'a>>>(path: R) -> Result<(), SysError> {
            let path = path.into();
            vfs_rmdir_at(&root_pathref(), path)
        }
    }

    pub use primitives::*;

    mod higher_level {
        use super::*;

        /// Pay attention that this might incur a huge heap allocation.
        pub fn vfs_read_to_string<'a, R: Into<PathResolution<'a>>>(
            path: R,
        ) -> Result<String, SysError> {
            let path = path.into();
            let file = vfs_open(path)?;
            let mut buf = Vec::new();
            let mut handle = file;
            handle.seek_set_checked(0)?;
            loop {
                let mut chunk = [0u8; 128];
                let n = handle.read(&mut chunk)?;
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }

            String::from_utf8(buf).map_err(|_| SysError::InvalidArgument)
        }
    }
    pub use higher_level::*;
}
pub use vfs_ops::*;

use crate::initcall::{InitCallLevel, run_initcalls};

pub fn register_filesystem_drivers() {
    unsafe {
        run_initcalls(InitCallLevel::Fs);
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use anemone_abi::fs::linux::mode as linux_mode;

    use super::*;
    use crate::{fs::namei::resolve_from_with_root, prelude::*};

    #[kunit]
    fn test_vfs_root_lookup() {
        let root = vfs_lookup(PathResolution::normal(&Path::new("/"))).unwrap();

        assert_eq!(root.to_string(), "/");
        assert_eq!(
            vfs_lookup(PathResolution::normal(&Path::new("/kunit-vfs-missing"))).unwrap_err(),
            SysError::NotFound
        );
    }

    #[kunit]
    fn test_vfs_create_lookup_and_cleanup() {
        let path = PathResolution::normal(&Path::new("/kunit-vfs-file"));

        assert_eq!(vfs_lookup(path).unwrap_err(), SysError::NotFound);

        let created = vfs_touch(path, InodePerm::all_rwx()).unwrap();
        let looked_up = vfs_lookup(path).unwrap();

        assert_eq!(created.to_string(), "/kunit-vfs-file");
        assert_eq!(looked_up.to_string(), "/kunit-vfs-file");
        assert_eq!(created.inode(), looked_up.inode());
        assert_eq!(
            vfs_touch(path, InodePerm::all_rwx()).unwrap_err(),
            SysError::AlreadyExists
        );

        vfs_unlink(path.target).unwrap();
        assert_eq!(vfs_lookup(path).unwrap_err(), SysError::NotFound);
    }

    #[kunit]
    fn test_vfs_mkdir_link_and_rmdir() {
        let dir_path = Path::new("/kunit-vfs-dir");
        let file_path = Path::new("/kunit-vfs-dir/file");
        let link_path = Path::new("/kunit-vfs-link");

        let dir = vfs_mkdir(dir_path, InodePerm::all_rwx()).unwrap();
        let file = vfs_touch(file_path, InodePerm::all_rwx()).unwrap();

        assert_eq!(dir.to_string(), "/kunit-vfs-dir");
        assert_eq!(file.to_string(), "/kunit-vfs-dir/file");
        assert_eq!(vfs_rmdir(dir_path).unwrap_err(), SysError::DirNotEmpty);

        vfs_link(file_path, link_path).unwrap();
        let linked = vfs_lookup(link_path).unwrap();

        assert_eq!(linked.to_string(), "/kunit-vfs-link");
        assert_eq!(linked.inode(), file.inode());
        assert_eq!(
            vfs_link(dir_path, Path::new("/kunit-vfs-dir-link")).unwrap_err(),
            SysError::IsDir
        );

        vfs_unlink(link_path).unwrap();
        vfs_unlink(file_path).unwrap();
        assert_eq!(vfs_lookup(link_path).unwrap_err(), SysError::NotFound);
        assert_eq!(vfs_lookup(file_path).unwrap_err(), SysError::NotFound);

        vfs_rmdir(dir_path).unwrap();
        assert_eq!(vfs_lookup(dir_path).unwrap_err(), SysError::NotFound);
    }

    #[kunit]
    fn test_vfs_symlink_relative_lookup_and_readlink() {
        let dir_path = Path::new("/kunit-vfs-sym-dir");
        let file_path = Path::new("/kunit-vfs-sym-dir/target");
        let link_path = Path::new("/kunit-vfs-sym-dir/link");

        vfs_mkdir(dir_path, InodePerm::all_rwx()).unwrap();
        let target = vfs_touch(file_path, InodePerm::all_rwx()).unwrap();
        let link = vfs_symlink(Path::new("target"), link_path).unwrap();

        assert_eq!(link.inode().ty(), InodeType::Symlink);
        assert_eq!(vfs_read_link(link_path).unwrap(), PathBuf::from("target"));
        assert_eq!(
            vfs_get_attr(link_path).unwrap().mode.ty(),
            InodeType::Regular
        );
        assert_eq!(
            vfs_lookup(PathResolution::new(
                link_path,
                ResolveFlags::DENY_LAST_SYMLINK
            ))
            .unwrap_err(),
            SysError::LinkEncountered
        );
        assert_eq!(
            vfs_get_attr(PathResolution::new(
                link_path,
                ResolveFlags::UNFOLLOW_LAST_SYMLINK
            ))
            .unwrap()
            .mode
            .ty(),
            InodeType::Symlink
        );

        let looked_up = vfs_lookup(link_path).unwrap();
        assert_eq!(looked_up.inode(), target.inode());
        assert_eq!(
            vfs_lookup(PathResolution::new(
                link_path,
                ResolveFlags::UNFOLLOW_LAST_SYMLINK
            ))
            .unwrap()
            .inode()
            .ty(),
            InodeType::Symlink
        );
        assert_eq!(
            vfs_read_link(link_path).unwrap(),
            vfs_lookup(PathResolution::new(
                link_path,
                ResolveFlags::UNFOLLOW_LAST_SYMLINK
            ))
            .unwrap()
            .inode()
            .read_link()
            .unwrap()
        );

        vfs_unlink(link_path).unwrap();
        vfs_unlink(file_path).unwrap();
        vfs_rmdir(dir_path).unwrap();
    }

    #[kunit]
    fn test_vfs_symlink_absolute_and_intermediate_resolution() {
        let dir_path = Path::new("/kunit-vfs-sym-abs-dir");
        let file_path = Path::new("/kunit-vfs-sym-abs-dir/file");
        let mid_link = Path::new("/kunit-vfs-sym-abs-mid");

        vfs_mkdir(dir_path, InodePerm::all_rwx()).unwrap();
        let target = vfs_touch(file_path, InodePerm::all_rwx()).unwrap();
        vfs_symlink(Path::new("/kunit-vfs-sym-abs-dir"), mid_link).unwrap();

        let resolved = vfs_lookup(Path::new("/kunit-vfs-sym-abs-mid/file")).unwrap();
        assert_eq!(resolved.inode(), target.inode());

        vfs_unlink(mid_link).unwrap();
        vfs_unlink(file_path).unwrap();
        vfs_rmdir(dir_path).unwrap();
    }

    #[kunit]
    fn test_vfs_symlink_relative_parent_traversal() {
        let dir_path = Path::new("/kunit-vfs-sym-parent-dir");
        let subdir_path = Path::new("/kunit-vfs-sym-parent-dir/subdir");
        let target_path = Path::new("/kunit-vfs-sym-parent-dir/target");
        let link_path = Path::new("/kunit-vfs-sym-parent-dir/subdir/up-link");

        vfs_mkdir(dir_path, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(subdir_path, InodePerm::all_rwx()).unwrap();
        let target = vfs_touch(target_path, InodePerm::all_rwx()).unwrap();
        vfs_symlink(Path::new("../target"), link_path).unwrap();

        assert_eq!(
            vfs_read_link(link_path).unwrap(),
            PathBuf::from("../target")
        );
        assert_eq!(vfs_lookup(link_path).unwrap().inode(), target.inode());

        vfs_unlink(link_path).unwrap();
        vfs_unlink(target_path).unwrap();
        vfs_rmdir(subdir_path).unwrap();
        vfs_rmdir(dir_path).unwrap();
    }

    #[kunit]
    fn test_vfs_symlink_resolution_flags_propagate_to_parent_lookup() {
        let dir_path = Path::new("/kunit-vfs-sym-flag-dir");
        let dir_link = Path::new("/kunit-vfs-sym-flag-link");
        let target_path = Path::new("/kunit-vfs-sym-flag-link/new-file");
        let resolved_target = Path::new("/kunit-vfs-sym-flag-dir/new-file");

        vfs_mkdir(dir_path, InodePerm::all_rwx()).unwrap();
        vfs_symlink(Path::new("/kunit-vfs-sym-flag-dir"), dir_link).unwrap();

        assert_eq!(
            vfs_lookup(PathResolution::new(
                dir_link,
                ResolveFlags::DENY_LAST_SYMLINK
            ))
            .unwrap_err(),
            SysError::LinkEncountered
        );
        assert_eq!(
            vfs_lookup(PathResolution::new(
                target_path,
                ResolveFlags::DENY_SYMLINKS
            ))
            .unwrap_err(),
            SysError::LinkEncountered
        );
        assert_eq!(
            vfs_lookup(PathResolution::new(
                dir_link,
                ResolveFlags::UNFOLLOW_LAST_SYMLINK
            ))
            .unwrap()
            .inode()
            .ty(),
            InodeType::Symlink
        );
        assert_eq!(
            vfs_touch(
                PathResolution::new(target_path, ResolveFlags::DENY_LAST_SYMLINK),
                InodePerm::all_rwx()
            )
            .unwrap_err(),
            SysError::LinkEncountered
        );
        let created = vfs_touch(target_path, InodePerm::all_rwx()).unwrap();
        assert_eq!(
            vfs_lookup(resolved_target).unwrap().to_string(),
            "/kunit-vfs-sym-flag-dir/new-file"
        );
        assert_eq!(
            created.inode(),
            vfs_lookup(resolved_target).unwrap().inode()
        );

        vfs_unlink(resolved_target).unwrap();
        vfs_unlink(dir_link).unwrap();
        vfs_rmdir(dir_path).unwrap();
    }

    #[kunit]
    fn test_vfs_symlink_absolute_target_crosses_mount_boundary() {
        let mountpoint = Path::new("/kunit-vfs-sym-mount");
        let host_target = Path::new("/kunit-vfs-sym-host-target");
        let link_path = Path::new("/kunit-vfs-sym-mount/host-link");

        vfs_mkdir(mountpoint, InodePerm::all_rwx()).unwrap();
        let host = vfs_touch(host_target, InodePerm::all_rwx()).unwrap();

        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            mountpoint,
        )
        .unwrap();
        vfs_symlink(Path::new("/kunit-vfs-sym-host-target"), link_path).unwrap();

        assert_eq!(
            vfs_read_link(link_path).unwrap(),
            PathBuf::from("/kunit-vfs-sym-host-target")
        );
        assert_eq!(vfs_lookup(link_path).unwrap().inode(), host.inode());

        vfs_unlink(link_path).unwrap();
        vfs_unmount(mountpoint).unwrap();
        vfs_unlink(host_target).unwrap();
        vfs_rmdir(mountpoint).unwrap();
    }

    #[kunit]
    fn test_resolve_from_root_uses_logical_root_for_absolute_symlinks() {
        let root_dir = Path::new("/kunit-vfs-chroot-root");
        let bin_dir = Path::new("/kunit-vfs-chroot-root/bin");
        let glibc_dir = Path::new("/kunit-vfs-chroot-root/glibc");
        let busybox_path = Path::new("/kunit-vfs-chroot-root/glibc/busybox");
        let sh_path = Path::new("/kunit-vfs-chroot-root/bin/sh");

        vfs_mkdir(root_dir, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(bin_dir, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(glibc_dir, InodePerm::all_rwx()).unwrap();
        let busybox = vfs_touch(busybox_path, InodePerm::all_rwx()).unwrap();
        vfs_symlink(Path::new("/glibc/busybox"), sh_path).unwrap();

        let logical_root = vfs_lookup(root_dir).unwrap();
        let resolved = resolve_from_with_root(
            &logical_root,
            &logical_root,
            Path::new("/bin/sh"),
            ResolveFlags::empty(),
        )
        .unwrap();

        assert_eq!(resolved.inode(), busybox.inode());

        vfs_unlink(sh_path).unwrap();
        vfs_unlink(busybox_path).unwrap();
        vfs_rmdir(glibc_dir).unwrap();
        vfs_rmdir(bin_dir).unwrap();
        vfs_rmdir(root_dir).unwrap();
    }

    #[kunit]
    fn test_resolve_from_root_clamps_parent_traversal_at_logical_root() {
        let root_dir = Path::new("/kunit-vfs-chroot-parent-root");
        let inner_target =
            Path::new("/kunit-vfs-chroot-parent-root/kunit-vfs-chroot-parent-target");
        let outer_target = Path::new("/kunit-vfs-chroot-parent-target");

        vfs_mkdir(root_dir, InodePerm::all_rwx()).unwrap();
        let inner = vfs_touch(inner_target, InodePerm::all_rwx()).unwrap();
        let outer = vfs_touch(outer_target, InodePerm::all_rwx()).unwrap();

        let logical_root = vfs_lookup(root_dir).unwrap();
        let resolved = resolve_from_with_root(
            &logical_root,
            &logical_root,
            Path::new("../kunit-vfs-chroot-parent-target"),
            ResolveFlags::empty(),
        )
        .unwrap();

        assert_eq!(resolved.inode(), inner.inode());
        assert_ne!(resolved.inode(), outer.inode());

        vfs_unlink(inner_target).unwrap();
        vfs_unlink(outer_target).unwrap();
        vfs_rmdir(root_dir).unwrap();
    }

    #[kunit]
    fn test_vfs_symlink_loop_limit_and_rmdir_nofollow() {
        let loop_a = Path::new("/kunit-vfs-loop-a");
        let loop_b = Path::new("/kunit-vfs-loop-b");
        let dir_path = Path::new("/kunit-vfs-sym-rmdir-dir");
        let dir_link = Path::new("/kunit-vfs-sym-rmdir-link");

        vfs_symlink(Path::new("kunit-vfs-loop-b"), loop_a).unwrap();
        vfs_symlink(Path::new("kunit-vfs-loop-a"), loop_b).unwrap();

        assert_eq!(vfs_lookup(loop_a).unwrap_err(), SysError::TooManyLinks);
        assert_eq!(
            vfs_lookup(PathResolution::new(loop_a, ResolveFlags::DENY_LAST_SYMLINK)).unwrap_err(),
            SysError::LinkEncountered
        );
        assert_eq!(
            vfs_lookup(PathResolution::new(
                loop_a,
                ResolveFlags::UNFOLLOW_LAST_SYMLINK
            ))
            .unwrap()
            .inode()
            .ty(),
            InodeType::Symlink
        );

        vfs_mkdir(dir_path, InodePerm::all_rwx()).unwrap();
        vfs_symlink(Path::new("/kunit-vfs-sym-rmdir-dir"), dir_link).unwrap();
        assert_eq!(vfs_rmdir(dir_link).unwrap_err(), SysError::NotDir);

        vfs_unlink(dir_link).unwrap();
        vfs_rmdir(dir_path).unwrap();
        vfs_unlink(loop_a).unwrap();
        vfs_unlink(loop_b).unwrap();
    }

    #[kunit]
    fn test_vfs_file_read_write_semantics() {
        let path = Path::new("/kunit-vfs-rw");
        let file = vfs_touch(path, InodePerm::all_rwx()).unwrap();

        let opened = vfs_open(path).unwrap();
        assert_eq!(opened.pos(), 0);

        assert_eq!(opened.write(b"hello").unwrap(), 5);
        assert_eq!(opened.pos(), 5);

        opened.seek_set_checked(2).unwrap();
        assert_eq!(opened.write(b"X").unwrap(), 1);
        assert_eq!(opened.pos(), 3);

        opened.seek_set_checked(8).unwrap();
        assert_eq!(opened.write(b"Z").unwrap(), 1);
        assert_eq!(opened.pos(), 9);

        opened.seek_set_checked(0).unwrap();
        let mut buf = [0u8; 9];
        assert_eq!(opened.read(&mut buf).unwrap(), 9);
        assert_eq!(&buf, b"heXlo\0\0\0Z");

        let second_handle = vfs_open(path).unwrap();
        assert_eq!(second_handle.pos(), 0);

        let mut prefix = [0u8; 4];
        assert_eq!(second_handle.read(&mut prefix).unwrap(), 4);
        assert_eq!(&prefix, b"heXl");

        let mut eof_buf = [0u8; 4];
        second_handle.seek_set_checked(32).unwrap();
        assert_eq!(second_handle.read(&mut eof_buf).unwrap(), 0);

        drop(second_handle);
        drop(opened);

        assert_eq!(file.inode().ty(), InodeType::Regular);
        vfs_unlink(path).unwrap();
        assert_eq!(vfs_lookup(path).unwrap_err(), SysError::NotFound);
    }

    #[kunit]
    fn test_vfs_get_attr_reports_basic_metadata() {
        let dir_path = Path::new("/kunit-vfs-attr-dir");
        let child_dir_path = Path::new("/kunit-vfs-attr-dir/subdir");
        let file_path = Path::new("/kunit-vfs-attr-dir/file");

        let dir = vfs_mkdir(dir_path, InodePerm::all_rwx()).unwrap();
        let dir_attr = vfs_get_attr(dir_path).unwrap();

        assert_eq!(dir_attr.ino, dir.inode().ino());
        assert_eq!(dir_attr.mode.ty(), InodeType::Dir);
        assert_eq!(
            dir_attr.mode.to_linux_mode(),
            linux_mode::S_IFDIR | InodePerm::all_rwx().bits() as u32
        );
        assert_eq!(dir_attr.nlink, 2);
        assert_eq!(dir_attr.uid, Uid::ROOT);
        assert_eq!(dir_attr.gid, Gid::ROOT);
        // dir size is filesystem-specific.
        assert_eq!(dir_attr.rdev, DeviceId::None);

        let file = vfs_touch(file_path, InodePerm::all_rwx()).unwrap();
        let file_attr = vfs_get_attr(file_path).unwrap();

        assert_eq!(file_attr.ino, file.inode().ino());
        assert_eq!(file_attr.mode.ty(), InodeType::Regular);
        assert_eq!(
            file_attr.mode.to_linux_mode(),
            linux_mode::S_IFREG | InodePerm::all_rwx().bits() as u32
        );
        assert_eq!(file_attr.nlink, 1);
        assert_eq!(file_attr.uid, Uid::ROOT);
        assert_eq!(file_attr.gid, Gid::ROOT);
        assert_eq!(file_attr.size, 0);
        assert_eq!(file_attr.rdev, DeviceId::None);

        vfs_mkdir(child_dir_path, InodePerm::all_rwx()).unwrap();
        assert_eq!(vfs_get_attr(dir_path).unwrap().nlink, 3);

        vfs_rmdir(child_dir_path).unwrap();
        assert_eq!(vfs_get_attr(dir_path).unwrap().nlink, 2);

        vfs_unlink(file_path).unwrap();
        vfs_rmdir(dir_path).unwrap();
    }

    #[kunit]
    fn test_vfs_get_attr_tracks_hard_link_counts() {
        let file_path = Path::new("/kunit-vfs-attr-link-src");
        let link_path = Path::new("/kunit-vfs-attr-link-dst");

        let created = vfs_touch(file_path, InodePerm::all_rwx()).unwrap();
        assert_eq!(vfs_get_attr(file_path).unwrap().nlink, 1);

        vfs_link(file_path, link_path).unwrap();

        let src_attr = vfs_get_attr(file_path).unwrap();
        let dst_attr = vfs_get_attr(link_path).unwrap();
        assert_eq!(src_attr.ino, created.inode().ino());
        assert_eq!(dst_attr.ino, created.inode().ino());
        assert_eq!(src_attr.nlink, 2);
        assert_eq!(dst_attr.nlink, 2);

        vfs_unlink(file_path).unwrap();

        let remaining = vfs_get_attr(link_path).unwrap();
        assert_eq!(remaining.ino, created.inode().ino());
        assert_eq!(remaining.nlink, 1);

        vfs_unlink(link_path).unwrap();
    }

    #[kunit]
    fn test_vfs_get_attr_tracks_size_after_writes() {
        let path = Path::new("/kunit-vfs-attr-size");

        vfs_touch(path, InodePerm::all_rwx()).unwrap();
        let opened = vfs_open(path).unwrap();

        let initial = vfs_get_attr(path).unwrap();
        assert_eq!(initial.size, 0);
        assert_eq!(initial.linux_blocks(), 0);

        assert_eq!(opened.write(b"abc").unwrap(), 3);
        let after_append = opened.get_attr().unwrap();
        assert_eq!(after_append.size, 3);
        assert_eq!(after_append.linux_blocks(), 1);
        assert_eq!(after_append.nlink, 1);

        opened.seek_set_checked(8).unwrap();
        assert_eq!(opened.write(b"z").unwrap(), 1);

        let after_hole = vfs_get_attr(path).unwrap();
        assert_eq!(after_hole.size, 9);
        assert_eq!(after_hole.linux_blocks(), 1);
        assert_eq!(after_hole.mode.ty(), InodeType::Regular);
        assert_eq!(after_hole.mode, initial.mode);

        drop(opened);
        vfs_unlink(path).unwrap();
    }
}
