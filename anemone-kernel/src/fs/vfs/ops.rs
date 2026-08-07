use crate::{
    fs::{
        mount_stack_top_at,
        namei::{materialize_child_dentry, resolve, resolve_parent},
        unmount,
    },
    prelude::*,
};

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

mod primitives {
    use crate::fs::{inode::RenameFlags, namei::resolve_parent_from};

    use super::*;

    fn init_new_inode_metadata(inode: &InodeRef, perm: InodePerm, uid: Uid, gid: Gid) {
        let ctime = realtime();

        inode.chown(Some(uid), Some(gid), ctime);
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

    /// Create an explicitly root-owned regular file in the visible namespace.
    pub fn vfs_touch_as_root<'a, R: Into<PathResolution<'a>>>(
        path: R,
        perm: InodePerm,
    ) -> Result<PathRef, SysError> {
        let path = path.into();
        let (parent, name) = resolve_parent(path.target, path.flags)?;
        vfs_touch_at(&parent, &name, perm, Uid::ROOT, Gid::ROOT)
    }

    pub fn vfs_touch_at(
        parent: &PathRef,
        name: &str,
        perm: InodePerm,
        uid: Uid,
        gid: Gid,
    ) -> Result<PathRef, SysError> {
        parent.mount().ensure_writable()?;

        let inode = parent.inode().touch(name, perm)?;
        init_new_inode_metadata(&inode, perm, uid, gid);

        let dentry = materialize_child_dentry(parent.dentry(), name, inode)?;

        Ok(PathRef::new(parent.mount().clone(), dentry))
    }

    pub fn vfs_make_node_at(
        parent: &PathRef,
        name: &str,
        description: MakeNodeDescription,
    ) -> Result<PathRef, SysError> {
        if parent.inode().ty() != InodeType::Dir {
            return Err(SysError::NotDir);
        }
        parent.mount().ensure_writable()?;

        let inode = parent.inode().make_node(name, description)?;
        let dentry = materialize_child_dentry(parent.dentry(), name, inode)?;

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

    pub fn vfs_get_attr<'a, R: Into<PathResolution<'a>>>(path: R) -> Result<InodeStat, SysError> {
        let path = path.into();
        resolve(path.target, path.flags)?.inode().get_attr()
    }

    /// Create an explicitly root-owned directory in the visible namespace.
    pub fn vfs_mkdir_as_root<'a, R: Into<PathResolution<'a>>>(
        path: R,
        perm: InodePerm,
    ) -> Result<PathRef, SysError> {
        let path = path.into();
        let (parent, name) = resolve_parent(path.target, path.flags)?;
        vfs_mkdir_at(&parent, &name, perm, Uid::ROOT, Gid::ROOT)
    }

    pub fn vfs_mkdir_at(
        parent: &PathRef,
        name: &str,
        perm: InodePerm,
        uid: Uid,
        gid: Gid,
    ) -> Result<PathRef, SysError> {
        parent.mount().ensure_writable()?;

        let inode = parent.inode().mkdir(name, perm)?;
        init_new_inode_metadata(&inode, perm, uid, gid);

        let dentry = materialize_child_dentry(parent.dentry(), name, inode)?;

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

    /// Create an explicitly root-owned symbolic link in the visible namespace.
    pub fn vfs_symlink_as_root<'a, R: Into<PathResolution<'a>>>(
        target: &Path,
        link_path: R,
    ) -> Result<PathRef, SysError> {
        let link_path = link_path.into();
        let (parent, name) = resolve_parent(link_path.target, link_path.flags)?;
        vfs_symlink_at(&parent, target, &name, Uid::ROOT, Gid::ROOT)
    }

    pub fn vfs_symlink_at(
        parent: &PathRef,
        target: &Path,
        name: &str,
        uid: Uid,
        gid: Gid,
    ) -> Result<PathRef, SysError> {
        if target.components().next().is_none() {
            // empty symlink is not allowed.
            return Err(SysError::InvalidArgument);
        }

        parent.mount().ensure_writable()?;
        let inode = parent.inode().symlink(name, target)?;
        init_new_inode_metadata(&inode, InodePerm::all_rwx(), uid, gid);
        let dentry = materialize_child_dentry(parent.dentry(), name, inode)?;

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

#[cfg(feature = "kunit")]
mod kunits {
    use anemone_abi::fs::linux::mode as linux_mode;

    use super::*;
    use crate::fs::namei::resolve_from_with_root;

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

        let created = vfs_touch_as_root(path, InodePerm::all_rwx()).unwrap();
        let looked_up = vfs_lookup(path).unwrap();

        assert_eq!(created.to_string(), "/kunit-vfs-file");
        assert_eq!(looked_up.to_string(), "/kunit-vfs-file");
        assert_eq!(created.inode(), looked_up.inode());
        assert_eq!(
            vfs_touch_as_root(path, InodePerm::all_rwx()).unwrap_err(),
            SysError::AlreadyExists
        );

        vfs_unlink(path.target).unwrap();
        assert_eq!(vfs_lookup(path).unwrap_err(), SysError::NotFound);
    }

    #[kunit]
    fn test_vfs_ramfs_directory_rename_and_cycle_guard() {
        let base_path = Path::new("/kunit-vfs-ramfs-rename");
        let source_path = Path::new("/kunit-vfs-ramfs-rename/source");
        let moved_path = Path::new("/kunit-vfs-ramfs-rename/moved");
        let child_path = Path::new("/kunit-vfs-ramfs-rename/moved/child");

        let base = vfs_mkdir_as_root(base_path, InodePerm::all_rwx()).unwrap();
        let source =
            vfs_mkdir_at(&base, "source", InodePerm::all_rwx(), Uid::ROOT, Gid::ROOT).unwrap();
        vfs_mkdir_at(&source, "child", InodePerm::all_rwx(), Uid::ROOT, Gid::ROOT).unwrap();

        let source_ref = vfs_lookup(source_path).unwrap();
        let base_ref = vfs_lookup(base_path).unwrap();
        vfs_rename_at(&source_ref, &base_ref, "moved", RenameFlags::empty()).unwrap();

        let moved_ref = vfs_lookup(moved_path).unwrap();
        assert_eq!(moved_ref.inode(), source.inode());
        let child_ref = vfs_lookup(child_path).unwrap();
        assert_eq!(
            vfs_rename_at(&moved_ref, &child_ref, "nested", RenameFlags::empty()),
            Err(SysError::InvalidArgument)
        );

        vfs_rmdir(child_path).unwrap();
        vfs_rmdir(moved_path).unwrap();
        vfs_rmdir(base_path).unwrap();
    }

    #[kunit]
    fn test_vfs_creation_uses_explicit_owner_and_permission() {
        let base_path = Path::new("/kunit-vfs-explicit-create");
        let base = vfs_mkdir_as_root(base_path, InodePerm::all_rwx()).unwrap();
        let uid = Uid::new(1234);
        let gid = Gid::new(5678);

        let dir_perm = InodePerm::IRUSR | InodePerm::IXUSR;
        let dir = vfs_mkdir_at(&base, "dir", dir_perm, uid, gid).unwrap();
        assert_eq!(dir.inode().perm(), dir_perm);
        assert_eq!(dir.inode().uid(), uid);
        assert_eq!(dir.inode().gid(), gid);

        let file_perm = InodePerm::IRUSR | InodePerm::IWGRP;
        let file = vfs_touch_at(&base, "file", file_perm, uid, gid).unwrap();
        assert_eq!(file.inode().perm(), file_perm);
        assert_eq!(file.inode().uid(), uid);
        assert_eq!(file.inode().gid(), gid);

        let link = vfs_symlink_at(&base, Path::new("file"), "link", uid, gid).unwrap();
        assert_eq!(link.inode().perm(), InodePerm::all_rwx());
        assert_eq!(link.inode().uid(), uid);
        assert_eq!(link.inode().gid(), gid);

        vfs_unlink_at(&base, Path::new("link")).unwrap();
        vfs_unlink_at(&base, Path::new("file")).unwrap();
        vfs_rmdir_at(&base, Path::new("dir")).unwrap();
        vfs_rmdir(base_path).unwrap();
    }

    #[kunit]
    fn test_vfs_mkdir_link_and_rmdir() {
        let dir_path = Path::new("/kunit-vfs-dir");
        let file_path = Path::new("/kunit-vfs-dir/file");
        let link_path = Path::new("/kunit-vfs-link");

        let dir = vfs_mkdir_as_root(dir_path, InodePerm::all_rwx()).unwrap();
        let file = vfs_touch_as_root(file_path, InodePerm::all_rwx()).unwrap();

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

        vfs_mkdir_as_root(dir_path, InodePerm::all_rwx()).unwrap();
        let target = vfs_touch_as_root(file_path, InodePerm::all_rwx()).unwrap();
        let link = vfs_symlink_as_root(Path::new("target"), link_path).unwrap();

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

        vfs_mkdir_as_root(dir_path, InodePerm::all_rwx()).unwrap();
        let target = vfs_touch_as_root(file_path, InodePerm::all_rwx()).unwrap();
        vfs_symlink_as_root(Path::new("/kunit-vfs-sym-abs-dir"), mid_link).unwrap();

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

        vfs_mkdir_as_root(dir_path, InodePerm::all_rwx()).unwrap();
        vfs_mkdir_as_root(subdir_path, InodePerm::all_rwx()).unwrap();
        let target = vfs_touch_as_root(target_path, InodePerm::all_rwx()).unwrap();
        vfs_symlink_as_root(Path::new("../target"), link_path).unwrap();

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

        vfs_mkdir_as_root(dir_path, InodePerm::all_rwx()).unwrap();
        vfs_symlink_as_root(Path::new("/kunit-vfs-sym-flag-dir"), dir_link).unwrap();

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
            vfs_touch_as_root(
                PathResolution::new(target_path, ResolveFlags::DENY_LAST_SYMLINK),
                InodePerm::all_rwx()
            )
            .unwrap_err(),
            SysError::LinkEncountered
        );
        let created = vfs_touch_as_root(target_path, InodePerm::all_rwx()).unwrap();
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

        vfs_mkdir_as_root(mountpoint, InodePerm::all_rwx()).unwrap();
        let host = vfs_touch_as_root(host_target, InodePerm::all_rwx()).unwrap();

        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            mountpoint,
        )
        .unwrap();
        vfs_symlink_as_root(Path::new("/kunit-vfs-sym-host-target"), link_path).unwrap();

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

        vfs_mkdir_as_root(root_dir, InodePerm::all_rwx()).unwrap();
        vfs_mkdir_as_root(bin_dir, InodePerm::all_rwx()).unwrap();
        vfs_mkdir_as_root(glibc_dir, InodePerm::all_rwx()).unwrap();
        let busybox = vfs_touch_as_root(busybox_path, InodePerm::all_rwx()).unwrap();
        vfs_symlink_as_root(Path::new("/glibc/busybox"), sh_path).unwrap();

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

        vfs_mkdir_as_root(root_dir, InodePerm::all_rwx()).unwrap();
        let inner = vfs_touch_as_root(inner_target, InodePerm::all_rwx()).unwrap();
        let outer = vfs_touch_as_root(outer_target, InodePerm::all_rwx()).unwrap();

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

        vfs_symlink_as_root(Path::new("kunit-vfs-loop-b"), loop_a).unwrap();
        vfs_symlink_as_root(Path::new("kunit-vfs-loop-a"), loop_b).unwrap();

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

        vfs_mkdir_as_root(dir_path, InodePerm::all_rwx()).unwrap();
        vfs_symlink_as_root(Path::new("/kunit-vfs-sym-rmdir-dir"), dir_link).unwrap();
        assert_eq!(vfs_rmdir(dir_link).unwrap_err(), SysError::NotDir);

        vfs_unlink(dir_link).unwrap();
        vfs_rmdir(dir_path).unwrap();
        vfs_unlink(loop_a).unwrap();
        vfs_unlink(loop_b).unwrap();
    }

    #[kunit]
    fn test_vfs_file_read_write_semantics() {
        let path = Path::new("/kunit-vfs-rw");
        let file = vfs_touch_as_root(path, InodePerm::all_rwx()).unwrap();

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

        let dir = vfs_mkdir_as_root(dir_path, InodePerm::all_rwx()).unwrap();
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

        let file = vfs_touch_as_root(file_path, InodePerm::all_rwx()).unwrap();
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

        vfs_mkdir_as_root(child_dir_path, InodePerm::all_rwx()).unwrap();
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

        let created = vfs_touch_as_root(file_path, InodePerm::all_rwx()).unwrap();
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

        vfs_touch_as_root(path, InodePerm::all_rwx()).unwrap();
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
