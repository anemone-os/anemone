//! Virtual file system and filesystem drivers.

use alloc::{string::ToString, sync::Arc};

// vfs infrastructure
mod anonymous;
mod dentry;
mod error;
mod file;
mod filesystem;
mod inode;
mod mount;
mod namei;
mod path;
mod superblock;

// filesystem drivers
mod devfs;
#[cfg(feature = "fs_ext4")]
mod ext4;
mod pipefs;
mod ramfs;

pub mod api;

pub use self::{
    anonymous::*,
    dentry::Dentry,
    error::FsError,
    file::{DirContext, DirEntry, File, FileOps},
    filesystem::{FileSystem, FileSystemFlags, FileSystemOps},
    inode::{
        DeviceId, Ino, InoIsZero, InodeMeta, InodeMode, InodeOps, InodePerm, InodeRef, InodeStat,
        InodeType, OpenedFile,
    },
    mount::{Mount, MountFlags, MountSource},
    namei::{resolve, resolve_from},
    path::PathRef,
    superblock::SuperBlock,
};

mod namespace {
    use crate::prelude::*;

    pub(super) struct NameSpace {
        root_path: Option<PathRef>,
        mounts: Vec<Arc<Mount>>,
        // tx_lock
    }

    impl NameSpace {
        pub(super) fn new() -> Self {
            Self {
                root_path: None,
                mounts: Vec::new(),
            }
        }

        pub(super) fn root_path(&self) -> Option<PathRef> {
            self.root_path.clone()
        }

        /// Mount a filesystem into this namespace. If `mountpoint` is `None`,
        /// the new mount becomes the root mount.
        fn mount(
            &mut self,
            fs: Arc<FileSystem>,
            source: MountSource,
            flags: MountFlags,
            mountpoint: Option<&PathRef>,
        ) -> Result<Arc<Mount>, FsError> {
            let (parent, mp_dentry) = match mountpoint {
                Some(pr) => (Some(pr.mount().clone()), Some(pr.dentry().clone())),
                None => (None, None),
            };

            if parent.is_none() && self.root_path.is_some() {
                // Mounting a new root when one already exists is not allowed.
                return Err(FsError::AlreadyExists);
            }

            let sb = fs.mount(source, flags)?;

            let root_inode = sb.root_inode().clone();
            let root_dentry = Arc::new(Dentry::new("/".to_string(), None, root_inode));

            let mnt = Arc::new(Mount::new(
                root_dentry,
                sb,
                parent.as_ref(),
                mp_dentry.as_ref(),
                flags,
            ));

            self.mounts.push(mnt.clone());

            if let Some(parent) = parent {
                parent.add_child(&mnt);
            }

            {
                if self.root_path.is_none() {
                    self.root_path = Some(PathRef::new(mnt.clone(), mnt.root().clone()));
                }
            }

            knoticeln!(
                "mounted filesystem: {} at {} with flags {:?}",
                fs.name(),
                mountpoint.map_or("none".to_string(), |mp| mp.to_string()),
                flags
            );

            Ok(mnt)
        }

        pub(super) fn mount_root(
            &mut self,
            fs: Arc<FileSystem>,
            source: MountSource,
            flags: MountFlags,
        ) -> Result<Arc<Mount>, FsError> {
            self.mount(fs, source, flags, None)
        }

        pub(super) fn mount_at(
            &mut self,
            fs: Arc<FileSystem>,
            source: MountSource,
            flags: MountFlags,
            mountpoint: &PathRef,
        ) -> Result<Arc<Mount>, FsError> {
            self.mount(fs, source, flags, Some(mountpoint))
        }

        /// Unmount a filesystem from this namespace.
        ///
        /// Unmounting root filesystem will fail.
        pub(super) fn unmount(&mut self, mount: &Arc<Mount>) -> Result<(), FsError> {
            // cannot unmount root
            let Some(parent) = mount.parent() else {
                return Err(FsError::InvalidArgument);
            };

            if mount.has_children() {
                knoticeln!("cannot unmount filesystem: superblock still has alive inodes");
                return Err(FsError::Busy);
            }

            let sb = mount.sb().clone();
            let sb_still_used = self
                .mounts
                .iter()
                .any(|m| !Arc::ptr_eq(m, mount) && Arc::ptr_eq(m.sb(), &sb));

            if !sb_still_used {
                if sb.has_alive_inode() {
                    knoticeln!("cannot unmount filesystem: superblock still has alive inodes");
                    return Err(FsError::Busy);
                }
            }

            // tear down the superblock if no other mount is using it.
            if !sb_still_used {
                // we can not recover. it's too complex.
                sb.try_evict_all()?;

                let fs = sb.fs().clone();
                fs.remove_sb(|s| Arc::ptr_eq(s, &sb));
                fs.kill_sb(sb);
            }

            parent
                .remove_child(&mount)
                .expect("mount should be a child of its parent");

            self.mounts.retain(|m| !Arc::ptr_eq(m, mount));

            knoticeln!("unmounted filesystem at {:?}", mount.mountpoint().unwrap());

            Ok(())
        }

        pub(super) fn mounts(&self) -> &[Arc<Mount>] {
            &self.mounts
        }
    }
}

// We prefer gathering all public APIs in this module, and keep the global state
// hidden in a singleton struct, which helps a lot to ensure lock ordering.
mod vfs {
    use crate::{fs::namespace::NameSpace, prelude::*};

    /// Virtual file system. Singleton instance.
    ///
    /// **LOCK ORDERING:**
    /// **`visible` -> `anonymous` -> `fs_list` → `mounts` → `root_mount`**
    struct VfsSubSys {
        /// Global namespace. Path resolution occurs here. For those filesystems
        /// that should be exposed to user space. e.g. disk-backed filesystems,
        /// devfs, sysfs, etc.
        visible: RwLock<NameSpace>,
        /// Anonymous namespace. For those kernel-internal pseudo file systems.
        /// e.g. pipefs, sockfs, etc.
        anonymous: RwLock<NameSpace>,
        fs_list: RwLock<Vec<Arc<FileSystem>>>,
    }

    static VFS: Lazy<VfsSubSys> = Lazy::new(|| VfsSubSys {
        visible: RwLock::new(NameSpace::new()),
        anonymous: RwLock::new(NameSpace::new()),
        fs_list: RwLock::new(Vec::new()),
    });

    /// Register a file system type.
    ///
    /// On success, returns an `Arc` to the registered `FileSystem`.
    pub fn register_filesystem(fs: &'static FileSystemOps) -> Result<Arc<FileSystem>, FsError> {
        let mut fs_list = VFS.fs_list.write();
        for existing in fs_list.iter() {
            if existing.name() == fs.name {
                return Err(FsError::AlreadyExists);
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
        flags: MountFlags,
        mountpoint: &PathRef,
    ) -> Result<Arc<Mount>, FsError> {
        let fs = get_filesystem(fs_name).ok_or(FsError::NotFound)?;

        VFS.visible.write().mount_at(fs, source, flags, mountpoint)
    }

    /// Mount a filesystem into visible namespace as the root mount.
    pub fn mount_root(
        fs_name: &str,
        source: MountSource,
        flags: MountFlags,
    ) -> Result<Arc<Mount>, FsError> {
        let fs = get_filesystem(fs_name).ok_or(FsError::NotFound)?;

        VFS.visible.write().mount_root(fs, source, flags)
    }

    /// **Called by anonymous filesystem driver. DO NOT TOUCH THIS.**
    pub fn mount_anonymous_root(anony_fs: Arc<FileSystem>) -> Result<Arc<Mount>, FsError> {
        VFS.anonymous
            .write()
            .mount_root(anony_fs, MountSource::Pseudo, MountFlags::empty())
    }

    /// Unmount a filesystem from visible namespace.
    pub fn unmount(mount: Arc<Mount>) -> Result<(), FsError> {
        VFS.visible.write().unmount(&mount)
    }

    /// Get the root [PathRef] of the visible namespace.
    ///
    /// # Panics
    ///
    /// Panics if the root mount has not been established yet. This should never
    /// happen after the initial filesystem has been mounted during boot.
    pub fn root_pathref() -> PathRef {
        VFS.visible
            .read()
            .root_path()
            .expect("root mount must be established")
    }

    /// Get the root [PathRef] of the anonymous namespace.
    pub fn anonymous_root_pathref() -> PathRef {
        VFS.anonymous
            .read()
            .root_path()
            .expect("anonymous root mount must be established")
    }

    /// For visible namespace.
    fn mounted_superblocks_for(namespace: &NameSpace) -> Vec<Arc<SuperBlock>> {
        let mounts = namespace.mounts();
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

    /// Called when the system is shutting down. This will flush all cached data
    /// to storage devices of file systems, if exist, and perform any necessary
    /// cleanup.
    pub unsafe fn on_shutdown() {
        fn sync_superblocks(namespace: &NameSpace) {
            for sb in mounted_superblocks_for(namespace) {
                if let Err(err) = sb.fs().sync_fs(&sb) {
                    kerrln!(
                        "failed to sync filesystem {} during shutdown: {:?}",
                        sb.fs().name(),
                        err
                    );
                }
            }
        }

        sync_superblocks(&VFS.anonymous.read());
        sync_superblocks(&VFS.visible.read());
    }
}
pub use vfs::*;

mod vfs_ops {
    use crate::{
        fs::{
            namei::{canonicalize_child, resolve, resolve_parent},
            unmount,
        },
        prelude::*,
    };

    /// These operations target the visible namespace.
    mod primitives {
        use crate::fs::namei::resolve_parent_from;

        use super::*;

        pub fn vfs_mount_at(
            fs_name: &str,
            source: MountSource,
            flags: MountFlags,
            mountpoint: &Path,
        ) -> Result<Arc<Mount>, FsError> {
            let mountpoint = resolve(mountpoint)?;
            mount_at(fs_name, source, flags, &mountpoint)
        }

        pub fn vfs_unmount(mountpoint: &Path) -> Result<(), FsError> {
            let mountpoint = resolve(mountpoint)?;
            // The path must point at the root of a mounted filesystem, not an
            // arbitrary entry inside one.
            let mount_root = mountpoint.mount().root();
            if !Arc::ptr_eq(mountpoint.dentry(), &mount_root) {
                return Err(FsError::NotMounted);
            }
            unmount(mountpoint.mount().clone())
        }

        pub fn vfs_lookup(path: &Path) -> Result<PathRef, FsError> {
            resolve(path)
        }

        pub fn vfs_create(path: &Path, mode: InodeMode) -> Result<PathRef, FsError> {
            vfs_create_at(&root_pathref(), path, mode)
        }

        pub fn vfs_create_at(
            dir: &PathRef,
            rel_path: &Path,
            mode: InodeMode,
        ) -> Result<PathRef, FsError> {
            let (parent, name) = resolve_parent_from(dir, rel_path)?;

            let inode = parent.inode().create(&name, mode)?;

            let dentry = canonicalize_child(parent.dentry(), &name, inode)?;

            Ok(PathRef::new(parent.mount().clone(), dentry))
        }

        pub fn vfs_open(path: &Path) -> Result<File, FsError> {
            let pathref = resolve(path)?;
            let inode = pathref.inode();
            let OpenedFile { file_ops, prv } = inode.open()?;

            Ok(File::new(pathref, file_ops, prv))
        }

        pub fn vfs_open_at(dir: &PathRef, rel_path: &Path) -> Result<File, FsError> {
            let pathref = resolve_from(dir, rel_path)?;
            let inode = pathref.inode();
            let OpenedFile { file_ops, prv } = inode.open()?;

            Ok(File::new(pathref, file_ops, prv))
        }

        pub fn vfs_get_attr(path: &Path) -> Result<InodeStat, FsError> {
            resolve(path)?.inode().get_attr()
        }

        pub fn vfs_mkdir(path: &Path, perm: InodePerm) -> Result<PathRef, FsError> {
            let (parent, name) = resolve_parent(path)?;

            let inode = parent.inode().mkdir(&name, perm)?;

            let dentry = canonicalize_child(parent.dentry(), &name, inode)?;

            Ok(PathRef::new(parent.mount().clone(), dentry))
        }

        pub fn vfs_link(old_path: &Path, new_path: &Path) -> Result<(), FsError> {
            let target = resolve(old_path)?;
            if target.inode().ty() == InodeType::Dir {
                return Err(FsError::IsDir);
            }

            let (parent, name) = resolve_parent(new_path)?;
            parent.inode().link(&name, target.inode())?;

            // insert the new dentry into parent.
            let _dentry = canonicalize_child(parent.dentry(), &name, target.inode().clone())?;

            Ok(())
        }

        pub fn vfs_unlink(path: &Path) -> Result<(), FsError> {
            vfs_unlink_at(&root_pathref(), path)
        }

        pub fn vfs_unlink_at(dir: &PathRef, rel_path: &Path) -> Result<(), FsError> {
            let (parent, name) = resolve_parent_from(dir, rel_path)?;
            parent.inode().unlink(&name)?;

            // remove the dentry from the cache to prevent stale lookups. the child
            // may never have been cached, which is not an error.
            match parent.dentry().remove_child(&name) {
                Ok(()) | Err(FsError::NotFound) => (),
                Err(err) => return Err(err),
            }

            Ok(())
        }

        pub fn vfs_rmdir_at(dir: &PathRef, rel_path: &Path) -> Result<(), FsError> {
            let target = resolve_from(dir, rel_path)?;
            if target.inode().ty() != InodeType::Dir {
                return Err(FsError::NotDir);
            }

            let (parent, name) = resolve_parent_from(dir, rel_path)?;

            if !Arc::ptr_eq(target.mount(), parent.mount()) {
                return Err(FsError::IsMountPoint);
            }

            parent.inode().rmdir(&name)?;

            // remove the dentry from the cache to prevent stale lookups. the child
            // may never have been cached, which is not an error.
            match parent.dentry().remove_child(&name) {
                Ok(()) | Err(FsError::NotFound) => (),
                Err(err) => return Err(err),
            }

            Ok(())
        }

        pub fn vfs_rmdir(path: &Path) -> Result<(), FsError> {
            vfs_rmdir_at(&root_pathref(), path)
        }
    }

    pub use primitives::*;

    mod higher_level {
        use super::*;

        /// Pay attention that this might incur a huge heap allocation.
        pub fn vfs_read_to_string(path: &Path) -> Result<String, FsError> {
            let file = vfs_open(path)?;
            let mut buf = Vec::new();
            let mut handle = file;
            handle.seek(0)?;
            loop {
                let mut chunk = [0u8; 128];
                let n = handle.read(&mut chunk)?;
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }

            String::from_utf8(buf).map_err(|_| FsError::InvalidArgument)
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
    use crate::{fs::namei::canonicalize_child, prelude::*};

    #[kunit]
    fn test_vfs_root_lookup() {
        let root = vfs_lookup(Path::new("/")).unwrap();

        assert_eq!(root.to_string(), "/");
        assert_eq!(
            vfs_lookup(Path::new("/kunit-vfs-missing")).unwrap_err(),
            FsError::NotFound
        );
    }

    #[kunit]
    fn test_vfs_create_lookup_and_cleanup() {
        let path = Path::new("/kunit-vfs-file");

        assert_eq!(vfs_lookup(path).unwrap_err(), FsError::NotFound);

        let created = vfs_create(path, InodeMode::new_with_all_perm(InodeType::Regular)).unwrap();
        let looked_up = vfs_lookup(path).unwrap();

        assert_eq!(created.to_string(), "/kunit-vfs-file");
        assert_eq!(looked_up.to_string(), "/kunit-vfs-file");
        assert_eq!(created.inode(), looked_up.inode());
        assert_eq!(
            vfs_create(path, InodeMode::new_with_all_perm(InodeType::Regular)).unwrap_err(),
            FsError::AlreadyExists
        );

        vfs_unlink(path).unwrap();
        assert_eq!(vfs_lookup(path).unwrap_err(), FsError::NotFound);
    }

    #[kunit]
    fn test_vfs_mkdir_link_and_rmdir() {
        let dir_path = Path::new("/kunit-vfs-dir");
        let file_path = Path::new("/kunit-vfs-dir/file");
        let link_path = Path::new("/kunit-vfs-link");

        let dir = vfs_mkdir(dir_path, InodePerm::all_rwx()).unwrap();
        let file = vfs_create(file_path, InodeMode::new_with_all_perm(InodeType::Regular)).unwrap();

        assert_eq!(dir.to_string(), "/kunit-vfs-dir");
        assert_eq!(file.to_string(), "/kunit-vfs-dir/file");
        assert_eq!(vfs_rmdir(dir_path).unwrap_err(), FsError::DirNotEmpty);

        vfs_link(file_path, link_path).unwrap();
        let linked = vfs_lookup(link_path).unwrap();

        assert_eq!(linked.to_string(), "/kunit-vfs-link");
        assert_eq!(linked.inode(), file.inode());
        assert_eq!(
            vfs_link(dir_path, Path::new("/kunit-vfs-dir-link")).unwrap_err(),
            FsError::IsDir
        );

        vfs_unlink(link_path).unwrap();
        vfs_unlink(file_path).unwrap();
        assert_eq!(vfs_lookup(link_path).unwrap_err(), FsError::NotFound);
        assert_eq!(vfs_lookup(file_path).unwrap_err(), FsError::NotFound);

        vfs_rmdir(dir_path).unwrap();
        assert_eq!(vfs_lookup(dir_path).unwrap_err(), FsError::NotFound);
    }

    #[kunit]
    fn test_vfs_file_read_write_semantics() {
        let path = Path::new("/kunit-vfs-rw");
        let file = vfs_create(path, InodeMode::new_with_all_perm(InodeType::Regular)).unwrap();

        let opened = vfs_open(path).unwrap();
        assert_eq!(opened.pos(), 0);

        assert_eq!(opened.write(b"hello").unwrap(), 5);
        assert_eq!(opened.pos(), 5);

        opened.seek(2).unwrap();
        assert_eq!(opened.write(b"X").unwrap(), 1);
        assert_eq!(opened.pos(), 3);

        opened.seek(8).unwrap();
        assert_eq!(opened.write(b"Z").unwrap(), 1);
        assert_eq!(opened.pos(), 9);

        opened.seek(0).unwrap();
        let mut buf = [0u8; 9];
        assert_eq!(opened.read(&mut buf).unwrap(), 9);
        assert_eq!(&buf, b"heXlo\0\0\0Z");

        let second_handle = vfs_open(path).unwrap();
        assert_eq!(second_handle.pos(), 0);

        let mut prefix = [0u8; 4];
        assert_eq!(second_handle.read(&mut prefix).unwrap(), 4);
        assert_eq!(&prefix, b"heXl");

        let mut eof_buf = [0u8; 4];
        second_handle.seek(32).unwrap();
        assert_eq!(second_handle.read(&mut eof_buf).unwrap(), 0);

        drop(second_handle);
        drop(opened);

        assert_eq!(file.inode().ty(), InodeType::Regular);
        vfs_unlink(path).unwrap();
        assert_eq!(vfs_lookup(path).unwrap_err(), FsError::NotFound);
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
        assert_eq!(dir_attr.uid, 0);
        assert_eq!(dir_attr.gid, 0);
        // dir size is filesystem-specific.
        assert_eq!(dir_attr.rdev, DeviceId::None);

        let file = vfs_create(file_path, InodeMode::new_with_all_perm(InodeType::Regular)).unwrap();
        let file_attr = vfs_get_attr(file_path).unwrap();

        assert_eq!(file_attr.ino, file.inode().ino());
        assert_eq!(file_attr.mode.ty(), InodeType::Regular);
        assert_eq!(
            file_attr.mode.to_linux_mode(),
            linux_mode::S_IFREG | InodePerm::all_rwx().bits() as u32
        );
        assert_eq!(file_attr.nlink, 1);
        assert_eq!(file_attr.uid, 0);
        assert_eq!(file_attr.gid, 0);
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

        let created =
            vfs_create(file_path, InodeMode::new_with_all_perm(InodeType::Regular)).unwrap();
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

        vfs_create(path, InodeMode::new_with_all_perm(InodeType::Regular)).unwrap();
        let opened = vfs_open(path).unwrap();

        let initial = vfs_get_attr(path).unwrap();
        assert_eq!(initial.size, 0);
        assert_eq!(initial.linux_blocks(), 0);

        assert_eq!(opened.write(b"abc").unwrap(), 3);
        let after_append = opened.get_attr().unwrap();
        assert_eq!(after_append.size, 3);
        assert_eq!(after_append.linux_blocks(), 1);
        assert_eq!(after_append.nlink, 1);

        opened.seek(8).unwrap();
        assert_eq!(opened.write(b"z").unwrap(), 1);

        let after_hole = vfs_get_attr(path).unwrap();
        assert_eq!(after_hole.size, 9);
        assert_eq!(after_hole.linux_blocks(), 1);
        assert_eq!(after_hole.mode.ty(), InodeType::Regular);
        assert_eq!(after_hole.mode, initial.mode);

        drop(opened);
        vfs_unlink(path).unwrap();
    }

    #[kunit]
    fn test_vfs_mount_overrides_mountpoint_and_restores_on_unmount() {
        let mountpoint = Path::new("/kunit-vfs-mnt");
        let lower = Path::new("/kunit-vfs-mnt/lower-file");
        let upper = Path::new("/kunit-vfs-mnt/upper-file");

        vfs_mkdir(mountpoint, InodePerm::all_rwx()).unwrap();
        vfs_create(lower, InodeMode::new_with_all_perm(InodeType::Regular)).unwrap();
        assert_eq!(
            vfs_lookup(lower).unwrap().to_string(),
            "/kunit-vfs-mnt/lower-file"
        );

        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountFlags::empty(),
            mountpoint,
        )
        .unwrap();

        assert_eq!(
            vfs_lookup(mountpoint).unwrap().to_string(),
            "/kunit-vfs-mnt"
        );
        assert_eq!(vfs_lookup(lower).unwrap_err(), FsError::NotFound);
        assert_eq!(vfs_rmdir(mountpoint).unwrap_err(), FsError::IsMountPoint);

        let file = vfs_create(upper, InodeMode::new_with_all_perm(InodeType::Regular)).unwrap();
        let reopened = vfs_open(upper).unwrap();
        assert_eq!(reopened.write(b"mounted").unwrap(), 7);
        reopened.seek(0).unwrap();

        let mut buf = [0u8; 7];
        assert_eq!(reopened.read(&mut buf).unwrap(), 7);
        assert_eq!(&buf, b"mounted");
        assert_eq!(vfs_unmount(upper).unwrap_err(), FsError::NotMounted);

        drop(reopened);
        drop(file);

        vfs_unmount(mountpoint).unwrap();

        assert_eq!(
            vfs_lookup(lower).unwrap().to_string(),
            "/kunit-vfs-mnt/lower-file"
        );
        assert_eq!(vfs_lookup(upper).unwrap_err(), FsError::NotFound);

        vfs_unlink(lower).unwrap();
        vfs_rmdir(mountpoint).unwrap();
    }

    #[kunit]
    fn test_vfs_unmount_busy_with_active_inode_ref() {
        let mountpoint = Path::new("/kunit-vfs-busy");
        let live = Path::new("/kunit-vfs-busy/live");

        vfs_mkdir(mountpoint, InodePerm::all_rwx()).unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountFlags::empty(),
            mountpoint,
        )
        .unwrap();

        let live_ref = vfs_create(live, InodeMode::new_with_all_perm(InodeType::Regular)).unwrap();
        assert_eq!(vfs_unmount(mountpoint).unwrap_err(), FsError::Busy);

        drop(live_ref);

        vfs_unmount(mountpoint).unwrap();
        vfs_rmdir(mountpoint).unwrap();
    }

    #[kunit]
    fn test_vfs_unmount_busy_with_child_mount() {
        let mountpoint = Path::new("/kunit-vfs-parent-mnt");
        let nested = Path::new("/kunit-vfs-parent-mnt/nested");

        vfs_mkdir(mountpoint, InodePerm::all_rwx()).unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountFlags::empty(),
            mountpoint,
        )
        .unwrap();

        vfs_mkdir(nested, InodePerm::all_rwx()).unwrap();
        vfs_mount_at("ramfs", MountSource::Pseudo, MountFlags::empty(), nested).unwrap();

        assert_eq!(vfs_unmount(mountpoint).unwrap_err(), FsError::Busy);

        vfs_unmount(nested).unwrap();
        vfs_rmdir(nested).unwrap();
        vfs_unmount(mountpoint).unwrap();
        vfs_rmdir(mountpoint).unwrap();
    }

    #[kunit]
    fn test_vfs_mount_dot_and_dotdot_traversal() {
        let mountpoint = Path::new("/kunit-vfs-walk");
        let host_sibling = Path::new("/kunit-vfs-host-sibling");
        let inner_dir = Path::new("/kunit-vfs-walk/sub");
        let inner_file = Path::new("/kunit-vfs-walk/sub/file");

        vfs_mkdir(mountpoint, InodePerm::all_rwx()).unwrap();
        vfs_create(
            host_sibling,
            InodeMode::new_with_all_perm(InodeType::Regular),
        )
        .unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountFlags::empty(),
            mountpoint,
        )
        .unwrap();

        assert_eq!(
            vfs_lookup(Path::new("/kunit-vfs-walk/."))
                .unwrap()
                .to_string(),
            "/kunit-vfs-walk"
        );

        vfs_mkdir(inner_dir, InodePerm::all_rwx()).unwrap();
        let inner =
            vfs_create(inner_file, InodeMode::new_with_all_perm(InodeType::Regular)).unwrap();

        assert_eq!(
            vfs_lookup(Path::new("/kunit-vfs-walk/./sub/./file"))
                .unwrap()
                .inode(),
            inner.inode()
        );
        assert_eq!(
            vfs_lookup(Path::new("/kunit-vfs-walk/sub/.."))
                .unwrap()
                .to_string(),
            "/kunit-vfs-walk"
        );
        assert_eq!(
            vfs_lookup(Path::new("/kunit-vfs-walk/sub/../sub/file"))
                .unwrap()
                .inode(),
            inner.inode()
        );
        assert_eq!(
            vfs_lookup(Path::new("/kunit-vfs-walk/../kunit-vfs-host-sibling"))
                .unwrap()
                .to_string(),
            "/kunit-vfs-host-sibling"
        );

        drop(inner);

        vfs_unmount(mountpoint).unwrap();
        vfs_unlink(host_sibling).unwrap();
        vfs_rmdir(mountpoint).unwrap();
    }

    #[kunit]
    fn test_vfs_multiple_mounts_are_isolated() {
        let left_mount = Path::new("/kunit-vfs-left-mnt");
        let right_mount = Path::new("/kunit-vfs-right-mnt");
        let left_file = Path::new("/kunit-vfs-left-mnt/file");
        let right_file = Path::new("/kunit-vfs-right-mnt/file");

        vfs_mkdir(left_mount, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(right_mount, InodePerm::all_rwx()).unwrap();

        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountFlags::empty(),
            left_mount,
        )
        .unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountFlags::empty(),
            right_mount,
        )
        .unwrap();

        let left = vfs_create(left_file, InodeMode::new_with_all_perm(InodeType::Regular)).unwrap();
        assert_eq!(vfs_lookup(right_file).unwrap_err(), FsError::NotFound);

        let right =
            vfs_create(right_file, InodeMode::new_with_all_perm(InodeType::Regular)).unwrap();
        assert_eq!(vfs_lookup(left_file).unwrap().inode(), left.inode());
        assert_eq!(vfs_lookup(right_file).unwrap().inode(), right.inode());

        drop(left);
        drop(right);

        vfs_unmount(left_mount).unwrap();
        assert_eq!(vfs_lookup(left_file).unwrap_err(), FsError::NotFound);
        assert_eq!(
            vfs_lookup(right_file).unwrap().to_string(),
            "/kunit-vfs-right-mnt/file"
        );

        vfs_unmount(right_mount).unwrap();
        vfs_rmdir(left_mount).unwrap();
        vfs_rmdir(right_mount).unwrap();
    }

    #[kunit]
    fn test_vfs_mount_cycle_stress() {
        const NROUNDS: usize = 8;
        const NFILES: usize = 8;

        let mountpoint = Path::new("/kunit-vfs-cycle");
        vfs_mkdir(mountpoint, InodePerm::all_rwx()).unwrap();

        for round in 0..NROUNDS {
            vfs_mount_at(
                "ramfs",
                MountSource::Pseudo,
                MountFlags::empty(),
                mountpoint,
            )
            .unwrap();

            for file_idx in 0..NFILES {
                let path = format!("/kunit-vfs-cycle/file-{round}-{file_idx}");
                let payload = format!("round-{round}-file-{file_idx}-payload");

                vfs_create(
                    Path::new(&path),
                    InodeMode::new_with_all_perm(InodeType::Regular),
                )
                .unwrap();

                let opened = vfs_open(Path::new(&path)).unwrap();
                assert_eq!(opened.write(payload.as_bytes()).unwrap(), payload.len());
                opened.seek(0).unwrap();

                let mut buf = vec![0u8; payload.len()];
                assert_eq!(opened.read(buf.as_mut_slice()).unwrap(), payload.len());
                assert_eq!(buf.as_slice(), payload.as_bytes());
            }

            vfs_unmount(mountpoint).unwrap();

            let vanished = format!("/kunit-vfs-cycle/file-{round}-0");
            assert_eq!(
                vfs_lookup(Path::new(&vanished)).unwrap_err(),
                FsError::NotFound
            );
        }

        vfs_rmdir(mountpoint).unwrap();
    }

    #[kunit]
    fn test_vfs_namespace_churn_stress_under_mount() {
        const NDIRS: usize = 4;
        const NFILES_PER_DIR: usize = 6;

        let mountpoint = Path::new("/kunit-vfs-churn");
        vfs_mkdir(mountpoint, InodePerm::all_rwx()).unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountFlags::empty(),
            mountpoint,
        )
        .unwrap();

        for dir_idx in 0..NDIRS {
            let dir = format!("/kunit-vfs-churn/dir-{dir_idx}");
            vfs_mkdir(Path::new(&dir), InodePerm::all_rwx()).unwrap();

            for file_idx in 0..NFILES_PER_DIR {
                let file = format!("{dir}/file-{file_idx}");
                let alias = format!("/kunit-vfs-churn/alias-{dir_idx}-{file_idx}");
                let payload = format!("dir-{dir_idx}-file-{file_idx}-payload");

                let created = vfs_create(
                    Path::new(&file),
                    InodeMode::new_with_all_perm(InodeType::Regular),
                )
                .unwrap();
                let opened = vfs_open(Path::new(&file)).unwrap();

                assert_eq!(opened.write(payload.as_bytes()).unwrap(), payload.len());
                opened.seek(0).unwrap();

                let mut buf = vec![0u8; payload.len()];
                assert_eq!(opened.read(buf.as_mut_slice()).unwrap(), payload.len());
                assert_eq!(buf.as_slice(), payload.as_bytes());

                if file_idx % 2 == 0 {
                    vfs_link(Path::new(&file), Path::new(&alias)).unwrap();
                    assert_eq!(
                        vfs_lookup(Path::new(&alias)).unwrap().inode(),
                        created.inode()
                    );
                    vfs_unlink(Path::new(&alias)).unwrap();
                }
            }
        }

        for dir_idx in (0..NDIRS).rev() {
            let dir = format!("/kunit-vfs-churn/dir-{dir_idx}");

            for file_idx in (0..NFILES_PER_DIR).rev() {
                let file = format!("{dir}/file-{file_idx}");
                vfs_unlink(Path::new(&file)).unwrap();
            }

            vfs_rmdir(Path::new(&dir)).unwrap();
        }

        vfs_unmount(mountpoint).unwrap();
        vfs_rmdir(mountpoint).unwrap();
    }

    #[kunit]
    fn test_vfs_direct_multi_mount_same_mountpoint_visibility_switch() {
        let mountpoint = Path::new("/kunit-vfs-direct-stack");
        let visible_file = Path::new("/kunit-vfs-direct-stack/visible");
        let hidden_file = Path::new("/kunit-vfs-direct-stack/hidden");

        let host_mp = vfs_mkdir(mountpoint, InodePerm::all_rwx()).unwrap();
        let first = mount_at("ramfs", MountSource::Pseudo, MountFlags::empty(), &host_mp).unwrap();
        let second = mount_at("ramfs", MountSource::Pseudo, MountFlags::empty(), &host_mp).unwrap();

        assert!(!Arc::ptr_eq(&first, &second));
        assert!(!Arc::ptr_eq(first.sb(), second.sb()));

        vfs_create(
            visible_file,
            InodeMode::new_with_all_perm(InodeType::Regular),
        )
        .unwrap();

        let second_root = PathRef::new(second.clone(), second.root().clone());
        let hidden_inode = second_root
            .inode()
            .create("hidden", InodeMode::new_with_all_perm(InodeType::Regular))
            .unwrap();
        canonicalize_child(second_root.dentry(), "hidden", hidden_inode.clone()).unwrap();

        assert_eq!(
            vfs_lookup(visible_file).unwrap().to_string(),
            "/kunit-vfs-direct-stack/visible"
        );
        assert_eq!(vfs_lookup(hidden_file).unwrap_err(), FsError::NotFound);

        unmount(first).unwrap();

        assert_eq!(vfs_lookup(visible_file).unwrap_err(), FsError::NotFound);
        assert_eq!(vfs_lookup(hidden_file).unwrap().inode(), &hidden_inode);

        drop(hidden_inode);
        drop(second_root);

        vfs_unmount(mountpoint).unwrap();
        vfs_rmdir(mountpoint).unwrap();
    }

    #[kunit]
    fn test_vfs_direct_multi_mount_stack_stress() {
        const NLAYERS: usize = 6;

        let mountpoint = Path::new("/kunit-vfs-direct-stack-stress");
        let host_mp = vfs_mkdir(mountpoint, InodePerm::all_rwx()).unwrap();
        let mut mounts = Vec::new();

        for layer in 0..NLAYERS {
            let mnt =
                mount_at("ramfs", MountSource::Pseudo, MountFlags::empty(), &host_mp).unwrap();
            let root = PathRef::new(mnt.clone(), mnt.root().clone());
            let name = format!("layer-{layer}");
            let inode = root
                .inode()
                .create(&name, InodeMode::new_with_all_perm(InodeType::Regular))
                .unwrap();
            canonicalize_child(root.dentry(), &name, inode).unwrap();
            mounts.push(mnt);
        }

        for layer in 0..NLAYERS {
            let path = format!("/kunit-vfs-direct-stack-stress/layer-{layer}");
            if layer == 0 {
                assert_eq!(vfs_lookup(Path::new(&path)).unwrap().to_string(), path);
            } else {
                assert_eq!(vfs_lookup(Path::new(&path)).unwrap_err(), FsError::NotFound);
            }
        }

        for layer in 0..NLAYERS {
            let current_name = format!("/kunit-vfs-direct-stack-stress/layer-{layer}");
            unmount(mounts[layer].clone()).unwrap();

            if layer + 1 < NLAYERS {
                let next_name = format!("/kunit-vfs-direct-stack-stress/layer-{}", layer + 1);
                assert_eq!(
                    vfs_lookup(Path::new(&current_name)).unwrap_err(),
                    FsError::NotFound
                );
                assert_eq!(
                    vfs_lookup(Path::new(&next_name)).unwrap().to_string(),
                    next_name
                );
            } else {
                assert_eq!(
                    vfs_lookup(Path::new(&current_name)).unwrap_err(),
                    FsError::NotFound
                );
            }
        }

        vfs_rmdir(mountpoint).unwrap();
    }
}
