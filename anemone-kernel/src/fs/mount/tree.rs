use super::{
    data::MountData,
    flags::MountAttrFlags,
    view::{Mount, MountSource},
};
use crate::prelude::*;

pub(in crate::fs) struct MountTree {
    /// Sleeping gate for ordinary topology writers. Early root publication has
    /// a separate spin-only path because fs initcalls run before `Mutex`
    /// locking is legal.
    tx_lock: Mutex<()>,
    inner: SpinLock<MountTreeInner>,
}

struct MountTreeInner {
    root_path: Option<PathRef>,
    mounts: Vec<Arc<Mount>>,
    placement_generation: u64,
}

impl MountTree {
    pub(in crate::fs) fn new() -> Self {
        Self {
            tx_lock: Mutex::new(()),
            inner: SpinLock::new(MountTreeInner {
                root_path: None,
                mounts: Vec::new(),
                placement_generation: 0,
            }),
        }
    }

    pub(in crate::fs) fn root_path(&self) -> Option<PathRef> {
        self.inner.lock_irqsave().root_path.clone()
    }

    pub(in crate::fs) fn placement_generation(&self) -> u64 {
        self.inner.lock_irqsave().placement_generation
    }

    /// Mount a filesystem into this tree. If `mountpoint` is `None`, the new
    /// mount becomes the root mount.
    fn mount(
        &self,
        fs: Arc<FileSystem>,
        source: MountSource,
        attrs: MountAttrFlags,
        data: MountData,
        mountpoint: Option<&PathRef>,
    ) -> Result<Arc<Mount>, SysError> {
        let _tx = self.tx_lock.lock();

        if mountpoint.is_none() && self.inner.lock_irqsave().root_path.is_some() {
            return Err(SysError::AlreadyExists);
        }

        if let Some(target) = mountpoint {
            let target_is_current = self.inner.lock_irqsave().path_is_current(target);
            if !target_is_current {
                knoticeln!(
                    "mount attach: target revalidation failed target={} reason=not-current",
                    target
                );
                return Err(SysError::Busy);
            }
        }

        let sb = fs.mount(source, data)?;
        let root_inode = sb.root_inode().clone();
        let root_dentry = Arc::new(Dentry::new("/".to_string(), None, root_inode));
        let mnt = Arc::new(Mount::new(root_dentry, sb.clone(), attrs));

        let stack_depth = {
            let mut inner = self.inner.lock_irqsave();
            match mountpoint {
                Some(target) => inner.attach_mount(&mnt, target)?,
                None => inner.attach_root(&mnt)?,
            }
        };

        knoticeln!(
            "mount attach: op={} target={} fstype={} attrs={:?} stack_depth={}",
            if mountpoint.is_some() { "new" } else { "root" },
            mountpoint.map_or("none".to_string(), |mp| mp.to_string()),
            fs.name(),
            attrs,
            stack_depth
        );

        Ok(mnt)
    }

    pub(in crate::fs) fn mount_early_pseudo_root(
        &self,
        fs: Arc<FileSystem>,
    ) -> Result<Arc<Mount>, SysError> {
        if self.inner.lock_irqsave().root_path.is_some() {
            return Err(SysError::AlreadyExists);
        }

        let source = MountSource::Pseudo;
        let attrs = MountAttrFlags::empty();
        let sb = fs.mount(source, MountData::Null)?;
        let root_inode = sb.root_inode().clone();
        let root_dentry = Arc::new(Dentry::new("/".to_string(), None, root_inode));
        let mnt = Arc::new(Mount::new(root_dentry, sb, attrs));

        // Anonymous root setup happens during fs initcalls, before local
        // IRQ/preemption state satisfies `Mutex::lock()`. This is an explicit
        // boot-only capability, not a fallback for arbitrary no-sleep or panic
        // contexts. At that point no task can race an ordinary mount
        // transaction, so publishing the first tree root under the inner spin
        // lock is the only permitted writer bypass. Later attach/unmount paths
        // must use `tx_lock`.
        let stack_depth = self.inner.lock_irqsave().attach_root(&mnt)?;

        knoticeln!(
            "mount attach: op=early-root target=none fstype={} attrs={:?} stack_depth={}",
            fs.name(),
            attrs,
            stack_depth
        );

        Ok(mnt)
    }

    pub(in crate::fs) fn mount_root(
        &self,
        fs: Arc<FileSystem>,
        source: MountSource,
        attrs: MountAttrFlags,
    ) -> Result<Arc<Mount>, SysError> {
        self.mount(fs, source, attrs, MountData::Null, None)
    }

    pub(in crate::fs) fn mount_at(
        &self,
        fs: Arc<FileSystem>,
        source: MountSource,
        attrs: MountAttrFlags,
        mountpoint: &PathRef,
    ) -> Result<Arc<Mount>, SysError> {
        self.mount(fs, source, attrs, MountData::Null, Some(mountpoint))
    }

    pub(in crate::fs) fn mount_at_with_data(
        &self,
        fs: Arc<FileSystem>,
        source: MountSource,
        attrs: MountAttrFlags,
        data: MountData,
        mountpoint: &PathRef,
    ) -> Result<Arc<Mount>, SysError> {
        self.mount(fs, source, attrs, data, Some(mountpoint))
    }

    pub(in crate::fs) fn remount_attrs(
        &self,
        target: &PathRef,
        attrs: MountAttrFlags,
    ) -> Result<(), SysError> {
        let _tx = self.tx_lock.lock();
        let old_attrs = self.inner.lock_irqsave().remount_attrs(target, attrs)?;

        knoticeln!(
            "mount remount: target={} old_attrs={:?} new_attrs={:?} scope=per-mount-view",
            target,
            old_attrs,
            attrs
        );

        Ok(())
    }

    pub(in crate::fs) fn bind_mount(
        &self,
        source: &PathRef,
        target: &PathRef,
        recursive: bool,
    ) -> Result<usize, SysError> {
        if source.inode().ty() != InodeType::Dir || target.inode().ty() != InodeType::Dir {
            return Err(SysError::NotDir);
        }

        let _tx = self.tx_lock.lock();
        let nodes = self
            .inner
            .lock_irqsave()
            .snapshot_bind_subtree(source, target, recursive)?;
        let clones = nodes
            .iter()
            .map(|node| {
                // Bind views intentionally share the source dentry tree and
                // superblock while receiving a fresh mount identity. Per-mount
                // attrs are copied at clone time and can diverge by remounting
                // the new view.
                Arc::new(Mount::new(
                    node.root.clone(),
                    node.source.sb().clone(),
                    node.source.attrs(),
                ))
            })
            .collect::<Vec<_>>();
        let clone_count = self
            .inner
            .lock_irqsave()
            .attach_bind_subtree(source, target, &nodes, &clones)?;

        knoticeln!(
            "mount bind: source={} target={} recursive={} clone_count={}",
            source,
            target,
            recursive,
            clone_count
        );

        Ok(clone_count)
    }

    pub(in crate::fs) fn move_mount(
        &self,
        source: &PathRef,
        target: &PathRef,
    ) -> Result<usize, SysError> {
        if source.inode().ty() != InodeType::Dir || target.inode().ty() != InodeType::Dir {
            return Err(SysError::NotDir);
        }

        let old_target = source.to_string();
        let new_target = target.to_string();
        let _tx = self.tx_lock.lock();
        let (subtree_size, stack_depth) = self.inner.lock_irqsave().move_mount(source, target)?;

        knoticeln!(
            "mount move: old_target={} new_target={} moved_mount={:?} subtree_size={} stack_depth={}",
            old_target,
            new_target,
            source.mount(),
            subtree_size,
            stack_depth
        );

        Ok(subtree_size)
    }

    pub(in crate::fs) fn make_private(
        &self,
        target: &PathRef,
        recursive: bool,
    ) -> Result<usize, SysError> {
        let _tx = self.tx_lock.lock();
        let subtree_size = self.inner.lock_irqsave().make_private(target, recursive)?;

        knoticeln!(
            "mount propagation: op=private target={} recursive={} subtree_size={} effect=already-private",
            target,
            recursive,
            subtree_size
        );

        Ok(subtree_size)
    }

    /// Unmount a filesystem from this tree.
    ///
    /// Unmounting root filesystem will fail.
    pub(in crate::fs) fn unmount(&self, mount: &Arc<Mount>) -> Result<(), SysError> {
        let _tx = self.tx_lock.lock();
        let plan = self.inner.lock_irqsave().plan_unmount(mount)?;

        if plan.last_view && plan.sb.has_alive_inode() {
            knoticeln!("cannot unmount filesystem: superblock still has alive inodes");
            return Err(SysError::Busy);
        }

        if plan.last_view && !plan.persistent_sb {
            plan.sb.try_evict_all()?;
        }

        let detached_last_view = self.inner.lock_irqsave().detach_mount(mount)?;
        assert_eq!(
            plan.last_view, detached_last_view,
            "mount tree must not change under the writer transaction lock"
        );

        if detached_last_view && !plan.persistent_sb {
            // Keep `tx_lock` held through final superblock teardown. New mounts
            // call `fs.mount()` under the same writer gate, so a last-view
            // superblock cannot be reused by `sget()` after the tree has
            // decided to kill it.
            plan.fs.remove_sb(|s| Arc::ptr_eq(s, &plan.sb));
            plan.fs.kill_sb(plan.sb);
        }

        knoticeln!("mount detach: op=unmount target={:?}", plan.mountpoint);

        Ok(())
    }

    pub(in crate::fs) fn top_child_at(
        &self,
        parent: &Arc<Mount>,
        mountpoint: &Arc<Dentry>,
    ) -> Option<Arc<Mount>> {
        let inner = self.inner.lock_irqsave();
        if !inner.contains_mount(parent) {
            return None;
        }
        parent.top_child_at(mountpoint)
    }

    pub(in crate::fs) fn mounts(&self) -> Vec<Arc<Mount>> {
        self.inner.lock_irqsave().mounts.clone()
    }
}

struct UnmountPlan {
    sb: Arc<SuperBlock>,
    fs: Arc<FileSystem>,
    mountpoint: Arc<Dentry>,
    persistent_sb: bool,
    last_view: bool,
}

struct BindSourceNode {
    source: Arc<Mount>,
    root: Arc<Dentry>,
    parent_index: Option<usize>,
    mountpoint: Option<Arc<Dentry>>,
}

impl MountTreeInner {
    fn bump_generation(&mut self) {
        self.placement_generation = self.placement_generation.wrapping_add(1);
    }

    fn contains_mount(&self, mount: &Arc<Mount>) -> bool {
        self.mounts.iter().any(|m| Arc::ptr_eq(m, mount))
    }

    fn path_is_current(&self, path: &PathRef) -> bool {
        self.contains_mount(path.mount())
            && path.mount().is_reachable()
            && path.mount().top_child_at(path.dentry()).is_none()
    }

    fn target_is_mount_root_current(&self, target: &PathRef) -> bool {
        Arc::ptr_eq(target.dentry(), target.mount().root()) && self.path_is_current(target)
    }

    fn remount_attrs(
        &mut self,
        target: &PathRef,
        attrs: MountAttrFlags,
    ) -> Result<MountAttrFlags, SysError> {
        if !Arc::ptr_eq(target.dentry(), target.mount().root()) {
            knoticeln!(
                "mount remount: target revalidation failed target={} reason=not-mount-root",
                target
            );
            return Err(SysError::InvalidArgument);
        }

        if !self.target_is_mount_root_current(target) {
            knoticeln!(
                "mount remount: target revalidation failed target={} reason=not-current",
                target
            );
            return Err(SysError::Busy);
        }

        let old_attrs = target.mount().attrs();
        // `RDONLY` is a per-mount view attribute in this RFC stage, not a
        // filesystem-instance reconfigure. Keep the release-store inside the
        // placement transaction so a successful remount publishes attrs only
        // for the revalidated live mount view.
        target.mount().set_attrs(attrs);
        Ok(old_attrs)
    }

    fn stack_depth_at(parent: &Arc<Mount>, mountpoint: &Arc<Dentry>) -> usize {
        let mut depth = 0;
        let mut cur_parent = parent.clone();
        let mut cur_mountpoint = mountpoint.clone();
        while let Some(child) = cur_parent.top_child_at(&cur_mountpoint) {
            depth += 1;
            cur_mountpoint = child.root().clone();
            cur_parent = child;
        }
        depth
    }

    fn attach_root(&mut self, mount: &Arc<Mount>) -> Result<usize, SysError> {
        if self.root_path.is_some() {
            return Err(SysError::AlreadyExists);
        }

        mount.mark_root();
        mount.sb().add_mount(mount);
        self.root_path = Some(PathRef::new(mount.clone(), mount.root().clone()));
        self.mounts.push(mount.clone());
        self.bump_generation();

        Ok(1)
    }

    fn dentry_is_at_or_under(dentry: &Arc<Dentry>, ancestor: &Arc<Dentry>) -> bool {
        let mut current = Some(dentry.clone());
        while let Some(candidate) = current {
            if Arc::ptr_eq(&candidate, ancestor) {
                return true;
            }
            current = candidate.parent();
        }
        false
    }

    fn collect_recursive_bind_children(
        nodes: &mut Vec<BindSourceNode>,
        parent_index: usize,
        parent_source: &Arc<Mount>,
        root_boundary: Option<&Arc<Dentry>>,
    ) {
        for child in parent_source.attached_children_snapshot() {
            let mountpoint = child
                .mountpoint()
                .expect("attached child mount must have a mountpoint");
            if root_boundary
                .is_some_and(|boundary| !Self::dentry_is_at_or_under(&mountpoint, boundary))
            {
                continue;
            }

            let child_index = nodes.len();
            nodes.push(BindSourceNode {
                source: child.clone(),
                root: child.root().clone(),
                parent_index: Some(parent_index),
                mountpoint: Some(mountpoint),
            });
            Self::collect_recursive_bind_children(nodes, child_index, &child, None);
        }
    }

    fn snapshot_bind_subtree(
        &self,
        source: &PathRef,
        target: &PathRef,
        recursive: bool,
    ) -> Result<Vec<BindSourceNode>, SysError> {
        if !self.path_is_current(source) {
            knoticeln!(
                "mount bind: source revalidation failed source={} reason=not-current recursive={}",
                source,
                recursive
            );
            return Err(SysError::Busy);
        }

        if !self.path_is_current(target) {
            knoticeln!(
                "mount bind: target revalidation failed target={} reason=not-current recursive={}",
                target,
                recursive
            );
            return Err(SysError::Busy);
        }

        let mut nodes = vec![BindSourceNode {
            source: source.mount().clone(),
            root: source.dentry().clone(),
            parent_index: None,
            mountpoint: None,
        }];
        if recursive {
            Self::collect_recursive_bind_children(
                &mut nodes,
                0,
                source.mount(),
                Some(source.dentry()),
            );
        }

        Ok(nodes)
    }

    fn attach_bind_clone(
        &mut self,
        mount: &Arc<Mount>,
        parent: &Arc<Mount>,
        mountpoint: &Arc<Dentry>,
    ) {
        mount.mark_attached(parent, mountpoint);
        mount.sb().add_mount(mount);
        self.mounts.push(mount.clone());
        parent.push_child(mount);
    }

    fn attach_bind_subtree(
        &mut self,
        source: &PathRef,
        target: &PathRef,
        nodes: &[BindSourceNode],
        clones: &[Arc<Mount>],
    ) -> Result<usize, SysError> {
        assert!(!nodes.is_empty(), "bind subtree must contain a root clone");
        assert_eq!(
            nodes.len(),
            clones.len(),
            "bind source snapshot and clone vector must stay aligned"
        );

        if !self.path_is_current(source) {
            knoticeln!(
                "mount bind: source revalidation failed source={} reason=not-current-before-publish",
                source
            );
            return Err(SysError::Busy);
        }

        if !self.path_is_current(target) {
            knoticeln!(
                "mount bind: target revalidation failed target={} reason=not-current-before-publish",
                target
            );
            return Err(SysError::Busy);
        }

        self.attach_bind_clone(&clones[0], target.mount(), target.dentry());
        for (index, node) in nodes.iter().enumerate().skip(1) {
            let parent_index = node
                .parent_index
                .expect("non-root bind clone must name a parent clone");
            let mountpoint = node
                .mountpoint
                .as_ref()
                .expect("non-root bind clone must name a mountpoint");
            self.attach_bind_clone(&clones[index], &clones[parent_index], mountpoint);
        }
        self.bump_generation();

        Ok(clones.len())
    }

    fn mount_is_at_or_under(candidate: &Arc<Mount>, ancestor: &Arc<Mount>) -> bool {
        let mut current = Some(candidate.clone());
        while let Some(mount) = current {
            if Arc::ptr_eq(&mount, ancestor) {
                return true;
            }
            current = mount.parent();
        }
        false
    }

    fn mount_subtree_size(mount: &Arc<Mount>) -> usize {
        1 + mount
            .attached_children_snapshot()
            .iter()
            .map(Self::mount_subtree_size)
            .sum::<usize>()
    }

    fn move_mount(
        &mut self,
        source: &PathRef,
        target: &PathRef,
    ) -> Result<(usize, usize), SysError> {
        if !Arc::ptr_eq(source.dentry(), source.mount().root()) {
            knoticeln!(
                "mount move: source revalidation failed source={} reason=not-mount-root",
                source
            );
            return Err(SysError::InvalidArgument);
        }

        if !self.target_is_mount_root_current(source) {
            knoticeln!(
                "mount move: source revalidation failed source={} reason=not-current-root",
                source
            );
            return Err(SysError::Busy);
        }

        if !self.path_is_current(target) {
            knoticeln!(
                "mount move: target revalidation failed target={} reason=not-current",
                target
            );
            return Err(SysError::Busy);
        }

        if source.mount().parent().is_none() {
            knoticeln!("mount move: rejecting root mount source={}", source);
            return Err(SysError::InvalidArgument);
        }

        if Self::mount_is_at_or_under(target.mount(), source.mount()) {
            knoticeln!(
                "mount move: rejecting cycle source={} target={} reason=target-inside-source-subtree",
                source,
                target
            );
            return Err(SysError::InvalidArgument);
        }

        let old_parent = source
            .mount()
            .parent()
            .expect("attached non-root mount must have a parent");
        let subtree_size = Self::mount_subtree_size(source.mount());

        old_parent
            .remove_child(source.mount())
            .expect("moved mount should be a child of its old parent");
        source
            .mount()
            .move_attached(target.mount(), target.dentry());
        target.mount().push_child(source.mount());
        self.bump_generation();

        Ok((
            subtree_size,
            Self::stack_depth_at(target.mount(), target.dentry()),
        ))
    }

    fn make_private(&self, target: &PathRef, recursive: bool) -> Result<usize, SysError> {
        if !self.path_is_current(target) {
            knoticeln!(
                "mount propagation: private target revalidation failed target={} recursive={} reason=not-current",
                target,
                recursive
            );
            return Err(SysError::Busy);
        }

        if recursive {
            Ok(Self::mount_subtree_size(target.mount()))
        } else {
            Ok(1)
        }
    }

    fn attach_mount(&mut self, mount: &Arc<Mount>, target: &PathRef) -> Result<usize, SysError> {
        if !self.path_is_current(target) {
            knoticeln!(
                "mount attach: target revalidation failed target={} reason=not-current",
                target
            );
            return Err(SysError::Busy);
        }

        let parent = target.mount().clone();
        let mountpoint = target.dentry().clone();
        mount.mark_attached(&parent, &mountpoint);
        mount.sb().add_mount(mount);
        self.mounts.push(mount.clone());
        parent.push_child(mount);
        self.bump_generation();

        Ok(Self::stack_depth_at(&parent, &mountpoint))
    }

    fn plan_unmount(&self, mount: &Arc<Mount>) -> Result<UnmountPlan, SysError> {
        if !self.contains_mount(mount) {
            return Err(SysError::NotMounted);
        }

        let Some(mountpoint) = mount.mountpoint() else {
            return Err(SysError::InvalidArgument);
        };

        if mount.has_attached_children() {
            knoticeln!("cannot unmount filesystem: mount has attached children");
            return Err(SysError::Busy);
        }

        let sb = mount.sb().clone();
        let fs = sb.fs().clone();
        let persistent_sb = fs.flags().contains(FileSystemFlags::PERSISTENT_SB);
        let last_view = !self
            .mounts
            .iter()
            .any(|m| !Arc::ptr_eq(m, mount) && Arc::ptr_eq(m.sb(), &sb));

        Ok(UnmountPlan {
            sb,
            fs,
            mountpoint,
            persistent_sb,
            last_view,
        })
    }

    fn detach_mount(&mut self, mount: &Arc<Mount>) -> Result<bool, SysError> {
        let plan = self.plan_unmount(mount)?;

        if plan.last_view && plan.sb.has_alive_inode() {
            knoticeln!("cannot unmount filesystem: superblock still has alive inodes");
            return Err(SysError::Busy);
        }

        let parent = mount
            .parent()
            .expect("attached non-root mount must have a parent");
        parent
            .remove_child(mount)
            .expect("mount should be a child of its parent");
        mount.mark_detached();
        self.mounts.retain(|m| !Arc::ptr_eq(m, mount));
        self.bump_generation();

        Ok(plan.last_view)
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use crate::prelude::*;

    #[kunit]
    fn test_vfs_mount_overrides_mountpoint_and_restores_on_unmount() {
        let mountpoint = Path::new("/kunit-vfs-mnt");
        let lower = Path::new("/kunit-vfs-mnt/lower-file");
        let upper = Path::new("/kunit-vfs-mnt/upper-file");

        vfs_mkdir(mountpoint, InodePerm::all_rwx()).unwrap();
        vfs_touch(lower, InodePerm::all_rwx()).unwrap();
        assert_eq!(
            vfs_lookup(lower).unwrap().to_string(),
            "/kunit-vfs-mnt/lower-file"
        );

        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            mountpoint,
        )
        .unwrap();

        assert_eq!(
            vfs_lookup(mountpoint).unwrap().to_string(),
            "/kunit-vfs-mnt"
        );
        assert_eq!(vfs_lookup(lower).unwrap_err(), SysError::NotFound);
        assert_eq!(vfs_rmdir(mountpoint).unwrap_err(), SysError::IsMountPoint);

        let file = vfs_touch(upper, InodePerm::all_rwx()).unwrap();
        let reopened = vfs_open(upper).unwrap();
        assert_eq!(reopened.write(b"mounted").unwrap(), 7);
        reopened.seek_set_checked(0).unwrap();

        let mut buf = [0u8; 7];
        assert_eq!(reopened.read(&mut buf).unwrap(), 7);
        assert_eq!(&buf, b"mounted");
        assert_eq!(vfs_unmount(upper).unwrap_err(), SysError::NotMounted);

        drop(reopened);
        drop(file);

        vfs_unmount(mountpoint).unwrap();

        assert_eq!(
            vfs_lookup(lower).unwrap().to_string(),
            "/kunit-vfs-mnt/lower-file"
        );
        assert_eq!(vfs_lookup(upper).unwrap_err(), SysError::NotFound);

        vfs_unlink(lower).unwrap();
        vfs_rmdir(mountpoint).unwrap();
    }

    #[kunit]
    fn test_vfs_root_mount_cannot_unmount() {
        let root = root_pathref();

        assert_eq!(
            unmount(root.mount().clone()).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            vfs_unmount(Path::new("/")).unwrap_err(),
            SysError::InvalidArgument
        );
    }

    #[kunit]
    fn test_vfs_direct_mount_rejects_covered_target_pathref() {
        let mountpoint = Path::new("/kunit-vfs-covered-target");

        let host_mp = vfs_mkdir(mountpoint, InodePerm::all_rwx()).unwrap();
        let first = mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            &host_mp,
        )
        .unwrap();

        assert_eq!(
            mount_at(
                "ramfs",
                MountSource::Pseudo,
                MountAttrFlags::empty(),
                &host_mp
            )
            .unwrap_err(),
            SysError::Busy
        );

        unmount(first).unwrap();
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
            MountAttrFlags::empty(),
            mountpoint,
        )
        .unwrap();

        let live_ref = vfs_touch(live, InodePerm::all_rwx()).unwrap();
        assert_eq!(vfs_unmount(mountpoint).unwrap_err(), SysError::Busy);

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
            MountAttrFlags::empty(),
            mountpoint,
        )
        .unwrap();

        vfs_mkdir(nested, InodePerm::all_rwx()).unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            nested,
        )
        .unwrap();

        assert_eq!(vfs_unmount(mountpoint).unwrap_err(), SysError::Busy);

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
        vfs_touch(host_sibling, InodePerm::all_rwx()).unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
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
        let inner = vfs_touch(inner_file, InodePerm::all_rwx()).unwrap();

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
            MountAttrFlags::empty(),
            left_mount,
        )
        .unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            right_mount,
        )
        .unwrap();

        let left = vfs_touch(left_file, InodePerm::all_rwx()).unwrap();
        assert_eq!(vfs_lookup(right_file).unwrap_err(), SysError::NotFound);

        let right = vfs_touch(right_file, InodePerm::all_rwx()).unwrap();
        assert_eq!(vfs_lookup(left_file).unwrap().inode(), left.inode());
        assert_eq!(vfs_lookup(right_file).unwrap().inode(), right.inode());

        drop(left);
        drop(right);

        vfs_unmount(left_mount).unwrap();
        assert_eq!(vfs_lookup(left_file).unwrap_err(), SysError::NotFound);
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
                MountAttrFlags::empty(),
                mountpoint,
            )
            .unwrap();

            for file_idx in 0..NFILES {
                let path = format!("/kunit-vfs-cycle/file-{round}-{file_idx}");
                let payload = format!("round-{round}-file-{file_idx}-payload");

                vfs_touch(Path::new(&path), InodePerm::all_rwx()).unwrap();

                let opened = vfs_open(Path::new(&path)).unwrap();
                assert_eq!(opened.write(payload.as_bytes()).unwrap(), payload.len());
                opened.seek_set_checked(0).unwrap();

                let mut buf = vec![0u8; payload.len()];
                assert_eq!(opened.read(buf.as_mut_slice()).unwrap(), payload.len());
                assert_eq!(buf.as_slice(), payload.as_bytes());
            }

            vfs_unmount(mountpoint).unwrap();

            let vanished = format!("/kunit-vfs-cycle/file-{round}-0");
            assert_eq!(
                vfs_lookup(Path::new(&vanished)).unwrap_err(),
                SysError::NotFound
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
            MountAttrFlags::empty(),
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

                let created = vfs_touch(Path::new(&file), InodePerm::all_rwx()).unwrap();
                let opened = vfs_open(Path::new(&file)).unwrap();

                assert_eq!(opened.write(payload.as_bytes()).unwrap(), payload.len());
                opened.seek_set_checked(0).unwrap();

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
    fn test_vfs_path_mount_same_mountpoint_uses_topmost_target() {
        let mountpoint = Path::new("/kunit-vfs-path-stack");
        let first_file = Path::new("/kunit-vfs-path-stack/first");
        let second_file = Path::new("/kunit-vfs-path-stack/second");

        vfs_mkdir(mountpoint, InodePerm::all_rwx()).unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            mountpoint,
        )
        .unwrap();
        vfs_touch(first_file, InodePerm::all_rwx()).unwrap();

        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            mountpoint,
        )
        .unwrap();
        vfs_touch(second_file, InodePerm::all_rwx()).unwrap();

        assert_eq!(vfs_lookup(first_file).unwrap_err(), SysError::NotFound);
        assert_eq!(
            vfs_lookup(second_file).unwrap().to_string(),
            "/kunit-vfs-path-stack/second"
        );

        vfs_unmount(mountpoint).unwrap();

        assert_eq!(vfs_lookup(second_file).unwrap_err(), SysError::NotFound);
        assert_eq!(
            vfs_lookup(first_file).unwrap().to_string(),
            "/kunit-vfs-path-stack/first"
        );

        vfs_unlink(first_file).unwrap();
        vfs_unmount(mountpoint).unwrap();
        vfs_rmdir(mountpoint).unwrap();
    }

    #[kunit]
    fn test_vfs_direct_multi_mount_same_mountpoint_visibility_switch() {
        let mountpoint = Path::new("/kunit-vfs-direct-stack");
        let visible_file = Path::new("/kunit-vfs-direct-stack/visible");
        let hidden_file = Path::new("/kunit-vfs-direct-stack/hidden");

        let host_mp = vfs_mkdir(mountpoint, InodePerm::all_rwx()).unwrap();
        let first = mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            &host_mp,
        )
        .unwrap();
        let first_root = PathRef::new(first.clone(), first.root().clone());
        let second = mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            &first_root,
        )
        .unwrap();

        assert!(!Arc::ptr_eq(&first, &second));
        assert!(!Arc::ptr_eq(first.sb(), second.sb()));

        let hidden_inode = first_root
            .inode()
            .touch("hidden", InodePerm::all_rwx())
            .unwrap();
        vfs_touch(visible_file, InodePerm::all_rwx()).unwrap();

        let second_root = PathRef::new(second.clone(), second.root().clone());
        assert_eq!(
            vfs_lookup(visible_file).unwrap().to_string(),
            "/kunit-vfs-direct-stack/visible"
        );
        assert_eq!(vfs_lookup(hidden_file).unwrap_err(), SysError::NotFound);

        vfs_unlink(visible_file).unwrap();
        unmount(second).unwrap();

        assert_eq!(vfs_lookup(visible_file).unwrap_err(), SysError::NotFound);
        assert_eq!(vfs_lookup(hidden_file).unwrap().inode(), &hidden_inode);

        drop(hidden_inode);
        drop(second_root);

        vfs_unlink(hidden_file).unwrap();
        vfs_unmount(mountpoint).unwrap();
        vfs_rmdir(mountpoint).unwrap();
    }

    #[kunit]
    fn test_vfs_direct_multi_mount_stack_stress() {
        const NLAYERS: usize = 6;

        let mountpoint = Path::new("/kunit-vfs-direct-stack-stress");
        let host_mp = vfs_mkdir(mountpoint, InodePerm::all_rwx()).unwrap();
        let mut mounts = Vec::new();
        let mut next_target = host_mp;

        for layer in 0..NLAYERS {
            let mnt = mount_at(
                "ramfs",
                MountSource::Pseudo,
                MountAttrFlags::empty(),
                &next_target,
            )
            .unwrap();
            let root = PathRef::new(mnt.clone(), mnt.root().clone());
            let name = format!("layer-{layer}");
            root.inode().touch(&name, InodePerm::all_rwx()).unwrap();
            next_target = root;
            mounts.push(mnt);
        }

        for layer in 0..NLAYERS {
            let path = format!("/kunit-vfs-direct-stack-stress/layer-{layer}");
            if layer + 1 == NLAYERS {
                assert_eq!(vfs_lookup(Path::new(&path)).unwrap().to_string(), path);
            } else {
                assert_eq!(
                    vfs_lookup(Path::new(&path)).unwrap_err(),
                    SysError::NotFound
                );
            }
        }

        for layer in (0..NLAYERS).rev() {
            let current_name = format!("/kunit-vfs-direct-stack-stress/layer-{layer}");
            vfs_unlink(Path::new(&current_name)).unwrap();
            unmount(mounts[layer].clone()).unwrap();

            if layer > 0 {
                let next_name = format!("/kunit-vfs-direct-stack-stress/layer-{}", layer - 1);
                assert_eq!(
                    vfs_lookup(Path::new(&current_name)).unwrap_err(),
                    SysError::NotFound
                );
                assert_eq!(
                    vfs_lookup(Path::new(&next_name)).unwrap().to_string(),
                    next_name
                );
            } else {
                assert_eq!(
                    vfs_lookup(Path::new(&current_name)).unwrap_err(),
                    SysError::NotFound
                );
            }
        }

        vfs_rmdir(mountpoint).unwrap();
    }

    #[kunit]
    fn test_vfs_mount_generation_bumps_on_attach_and_detach() {
        let mountpoint = Path::new("/kunit-vfs-generation");
        let host_mp = vfs_mkdir(mountpoint, InodePerm::all_rwx()).unwrap();

        let before_mount = mount_placement_generation();
        let mnt = mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            &host_mp,
        )
        .unwrap();
        let after_mount = mount_placement_generation();
        assert_ne!(before_mount, after_mount);

        unmount(mnt).unwrap();
        let after_unmount = mount_placement_generation();
        assert_ne!(after_mount, after_unmount);

        vfs_rmdir(mountpoint).unwrap();
    }

    #[kunit]
    fn test_vfs_plain_bind_creates_independent_mount_view() {
        let source_dir = Path::new("/kunit-vfs-bind-src");
        let source_file = Path::new("/kunit-vfs-bind-src/file");
        let target_dir = Path::new("/kunit-vfs-bind-target");
        let target_file = Path::new("/kunit-vfs-bind-target/file");

        vfs_mkdir(source_dir, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(target_dir, InodePerm::all_rwx()).unwrap();
        let source_file_ref = vfs_touch(source_file, InodePerm::all_rwx()).unwrap();

        let source = vfs_lookup(source_dir).unwrap();
        let target = vfs_lookup(target_dir).unwrap();
        assert_eq!(bind_mount(&source, &target, false).unwrap(), 1);

        let bound_root = vfs_lookup(target_dir).unwrap();
        assert!(!Arc::ptr_eq(bound_root.mount(), source.mount()));
        assert!(Arc::ptr_eq(bound_root.mount().sb(), source.mount().sb()));
        assert!(Arc::ptr_eq(bound_root.dentry(), source.dentry()));
        assert_eq!(bound_root.to_string(), "/kunit-vfs-bind-target");
        assert_eq!(
            vfs_lookup(target_file).unwrap().inode(),
            source_file_ref.inode()
        );

        vfs_unmount(target_dir).unwrap();
        assert_eq!(
            vfs_lookup(source_file).unwrap().inode(),
            source_file_ref.inode()
        );

        drop(source_file_ref);
        vfs_unlink(source_file).unwrap();
        vfs_rmdir(source_dir).unwrap();
        vfs_rmdir(target_dir).unwrap();
    }

    #[kunit]
    fn test_vfs_plain_bind_does_not_clone_child_mounts() {
        let source_dir = Path::new("/kunit-vfs-bind-plain-src");
        let nested = Path::new("/kunit-vfs-bind-plain-src/nested");
        let nested_file = Path::new("/kunit-vfs-bind-plain-src/nested/child-file");
        let target_dir = Path::new("/kunit-vfs-bind-plain-target");
        let target_nested_file = Path::new("/kunit-vfs-bind-plain-target/nested/child-file");

        vfs_mkdir(source_dir, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(nested, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(target_dir, InodePerm::all_rwx()).unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            nested,
        )
        .unwrap();
        let child_file = vfs_touch(nested_file, InodePerm::all_rwx()).unwrap();

        let source = vfs_lookup(source_dir).unwrap();
        let target = vfs_lookup(target_dir).unwrap();
        assert_eq!(bind_mount(&source, &target, false).unwrap(), 1);

        assert_eq!(
            vfs_lookup(target_nested_file).unwrap_err(),
            SysError::NotFound
        );

        vfs_unmount(target_dir).unwrap();
        drop(child_file);
        vfs_unlink(nested_file).unwrap();
        vfs_unmount(nested).unwrap();
        vfs_rmdir(nested).unwrap();
        vfs_rmdir(source_dir).unwrap();
        vfs_rmdir(target_dir).unwrap();
    }

    #[kunit]
    fn test_vfs_recursive_bind_clones_child_mounts() {
        let source_dir = Path::new("/kunit-vfs-rbind-src");
        let nested = Path::new("/kunit-vfs-rbind-src/nested");
        let nested_file = Path::new("/kunit-vfs-rbind-src/nested/child-file");
        let target_dir = Path::new("/kunit-vfs-rbind-target");
        let target_nested = Path::new("/kunit-vfs-rbind-target/nested");
        let target_nested_file = Path::new("/kunit-vfs-rbind-target/nested/child-file");

        vfs_mkdir(source_dir, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(nested, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(target_dir, InodePerm::all_rwx()).unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            nested,
        )
        .unwrap();
        let child_file = vfs_touch(nested_file, InodePerm::all_rwx()).unwrap();

        let source = vfs_lookup(source_dir).unwrap();
        let source_child = vfs_lookup(nested).unwrap();
        let target = vfs_lookup(target_dir).unwrap();
        assert_eq!(bind_mount(&source, &target, true).unwrap(), 2);

        let bound_child = vfs_lookup(target_nested).unwrap();
        assert!(!Arc::ptr_eq(bound_child.mount(), source_child.mount()));
        assert!(Arc::ptr_eq(
            bound_child.mount().sb(),
            source_child.mount().sb()
        ));
        assert_eq!(
            vfs_lookup(target_nested_file).unwrap().inode(),
            child_file.inode()
        );

        vfs_unmount(target_nested).unwrap();
        vfs_unmount(target_dir).unwrap();
        drop(bound_child);
        drop(source_child);
        drop(child_file);
        vfs_unlink(nested_file).unwrap();
        vfs_unmount(nested).unwrap();
        vfs_rmdir(nested).unwrap();
        vfs_rmdir(source_dir).unwrap();
        vfs_rmdir(target_dir).unwrap();
    }

    #[kunit]
    fn test_vfs_bind_remount_readonly_does_not_pollute_siblings() {
        let source_dir = Path::new("/kunit-vfs-bind-ro-src");
        let source_file = Path::new("/kunit-vfs-bind-ro-src/file");
        let ro_dir = Path::new("/kunit-vfs-bind-ro-target");
        let ro_file = Path::new("/kunit-vfs-bind-ro-target/file");
        let rw_dir = Path::new("/kunit-vfs-bind-rw-target");
        let rw_file = Path::new("/kunit-vfs-bind-rw-target/file");

        vfs_mkdir(source_dir, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(ro_dir, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(rw_dir, InodePerm::all_rwx()).unwrap();
        let file_ref = vfs_touch(source_file, InodePerm::all_rwx()).unwrap();

        let source = vfs_lookup(source_dir).unwrap();
        let ro_target = vfs_lookup(ro_dir).unwrap();
        let rw_target = vfs_lookup(rw_dir).unwrap();
        bind_mount(&source, &ro_target, false).unwrap();
        bind_mount(&source, &rw_target, false).unwrap();

        let ro_root = vfs_lookup(ro_dir).unwrap();
        remount_attrs(&ro_root, MountAttrFlags::RDONLY).unwrap();

        assert_eq!(
            vfs_open(ro_file).unwrap().write(b"ro").unwrap_err(),
            SysError::ReadOnlyFs
        );
        assert_eq!(vfs_open(source_file).unwrap().write(b"src").unwrap(), 3);
        assert_eq!(vfs_open(rw_file).unwrap().write(b"rw").unwrap(), 2);

        remount_attrs(&ro_root, MountAttrFlags::empty()).unwrap();
        vfs_unmount(ro_dir).unwrap();
        vfs_unmount(rw_dir).unwrap();
        drop(file_ref);
        vfs_unlink(source_file).unwrap();
        vfs_rmdir(source_dir).unwrap();
        vfs_rmdir(ro_dir).unwrap();
        vfs_rmdir(rw_dir).unwrap();
    }

    #[kunit]
    fn test_vfs_bind_rejects_file_source_or_target() {
        let source_dir = Path::new("/kunit-vfs-bind-file-src-dir");
        let source_file = Path::new("/kunit-vfs-bind-file-src-dir/file");
        let target_dir = Path::new("/kunit-vfs-bind-file-target-dir");
        let target_file = Path::new("/kunit-vfs-bind-file-target-dir/file");

        vfs_mkdir(source_dir, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(target_dir, InodePerm::all_rwx()).unwrap();
        let source_file_ref = vfs_touch(source_file, InodePerm::all_rwx()).unwrap();
        let target_file_ref = vfs_touch(target_file, InodePerm::all_rwx()).unwrap();

        let source_file_path = vfs_lookup(source_file).unwrap();
        let target = vfs_lookup(target_dir).unwrap();
        assert_eq!(
            bind_mount(&source_file_path, &target, false).unwrap_err(),
            SysError::NotDir
        );

        let source = vfs_lookup(source_dir).unwrap();
        let target_file_path = vfs_lookup(target_file).unwrap();
        assert_eq!(
            bind_mount(&source, &target_file_path, false).unwrap_err(),
            SysError::NotDir
        );

        drop(source_file_ref);
        drop(target_file_ref);
        vfs_unlink(source_file).unwrap();
        vfs_unlink(target_file).unwrap();
        vfs_rmdir(source_dir).unwrap();
        vfs_rmdir(target_dir).unwrap();
    }

    #[kunit]
    fn test_vfs_bind_revalidates_stale_source_and_target() {
        let stale_source_dir = Path::new("/kunit-vfs-bind-stale-source");
        let live_source_dir = Path::new("/kunit-vfs-bind-live-source");
        let stale_target_dir = Path::new("/kunit-vfs-bind-stale-target");
        let live_target_dir = Path::new("/kunit-vfs-bind-live-target");

        let stale_source = vfs_mkdir(stale_source_dir, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(live_source_dir, InodePerm::all_rwx()).unwrap();
        let stale_target = vfs_mkdir(stale_target_dir, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(live_target_dir, InodePerm::all_rwx()).unwrap();

        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            stale_source_dir,
        )
        .unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            stale_target_dir,
        )
        .unwrap();

        let live_source = vfs_lookup(live_source_dir).unwrap();
        let live_target = vfs_lookup(live_target_dir).unwrap();
        assert_eq!(
            bind_mount(&stale_source, &live_target, false).unwrap_err(),
            SysError::Busy
        );
        assert_eq!(
            bind_mount(&live_source, &stale_target, true).unwrap_err(),
            SysError::Busy
        );

        vfs_unmount(stale_source_dir).unwrap();
        vfs_unmount(stale_target_dir).unwrap();
        vfs_rmdir(stale_source_dir).unwrap();
        vfs_rmdir(live_source_dir).unwrap();
        vfs_rmdir(stale_target_dir).unwrap();
        vfs_rmdir(live_target_dir).unwrap();
    }

    #[kunit]
    fn test_vfs_bind_root_parent_and_path_rendering_use_target_boundary() {
        let source_dir = Path::new("/kunit-vfs-bind-root-src");
        let source_subdir = Path::new("/kunit-vfs-bind-root-src/sub");
        let target_dir = Path::new("/kunit-vfs-bind-root-target");

        vfs_mkdir(source_dir, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(source_subdir, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(target_dir, InodePerm::all_rwx()).unwrap();

        let source = vfs_lookup(source_subdir).unwrap();
        let target = vfs_lookup(target_dir).unwrap();
        bind_mount(&source, &target, false).unwrap();

        assert_eq!(
            vfs_lookup(target_dir).unwrap().to_string(),
            "/kunit-vfs-bind-root-target"
        );
        assert_eq!(
            vfs_lookup(Path::new("/kunit-vfs-bind-root-target/.."))
                .unwrap()
                .to_string(),
            "/"
        );

        vfs_unmount(target_dir).unwrap();
        vfs_rmdir(source_subdir).unwrap();
        vfs_rmdir(source_dir).unwrap();
        vfs_rmdir(target_dir).unwrap();
    }

    #[kunit]
    fn test_vfs_move_mount_preserves_identity_attrs_and_generation() {
        let source_dir = Path::new("/kunit-vfs-move-src");
        let source_file = Path::new("/kunit-vfs-move-src/file");
        let target_dir = Path::new("/kunit-vfs-move-target");
        let target_file = Path::new("/kunit-vfs-move-target/file");

        vfs_mkdir(source_dir, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(target_dir, InodePerm::all_rwx()).unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            source_dir,
        )
        .unwrap();
        vfs_touch(source_file, InodePerm::all_rwx()).unwrap();

        let source = vfs_lookup(source_dir).unwrap();
        let target = vfs_lookup(target_dir).unwrap();
        remount_attrs(&source, MountAttrFlags::RDONLY).unwrap();

        let moved_mount = source.mount().clone();
        let before_move = mount_placement_generation();
        assert_eq!(move_mount(&source, &target).unwrap(), 1);
        let after_move = mount_placement_generation();
        assert_ne!(before_move, after_move);

        let moved_root = vfs_lookup(target_dir).unwrap();
        assert!(Arc::ptr_eq(moved_root.mount(), &moved_mount));
        assert_eq!(moved_root.to_string(), "/kunit-vfs-move-target");
        assert_eq!(
            moved_root.mount().attrs(),
            MountAttrFlags::RDONLY,
            "move must preserve per-mount attrs on the same view"
        );
        assert_eq!(
            vfs_lookup(source_file).unwrap_err(),
            SysError::NotFound,
            "old mountpoint must no longer expose moved subtree"
        );
        assert_eq!(
            vfs_lookup(target_file).unwrap().to_string(),
            "/kunit-vfs-move-target/file"
        );

        remount_attrs(&moved_root, MountAttrFlags::empty()).unwrap();
        drop(moved_root);
        drop(moved_mount);
        drop(target);
        drop(source);
        vfs_unlink(target_file).unwrap();
        vfs_unmount(target_dir).unwrap();
        vfs_rmdir(source_dir).unwrap();
        vfs_rmdir(target_dir).unwrap();
    }

    #[kunit]
    fn test_vfs_move_mount_preserves_child_subtree() {
        let source_dir = Path::new("/kunit-vfs-move-tree-src");
        let nested_dir = Path::new("/kunit-vfs-move-tree-src/nested");
        let nested_file = Path::new("/kunit-vfs-move-tree-src/nested/file");
        let target_dir = Path::new("/kunit-vfs-move-tree-target");
        let target_nested = Path::new("/kunit-vfs-move-tree-target/nested");
        let target_file = Path::new("/kunit-vfs-move-tree-target/nested/file");

        vfs_mkdir(source_dir, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(target_dir, InodePerm::all_rwx()).unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            source_dir,
        )
        .unwrap();
        vfs_mkdir(nested_dir, InodePerm::all_rwx()).unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            nested_dir,
        )
        .unwrap();
        let child_file = vfs_touch(nested_file, InodePerm::all_rwx()).unwrap();

        let source = vfs_lookup(source_dir).unwrap();
        let child_mount = vfs_lookup(nested_dir).unwrap().mount().clone();
        let target = vfs_lookup(target_dir).unwrap();
        assert_eq!(move_mount(&source, &target).unwrap(), 2);

        let moved_child = vfs_lookup(target_nested).unwrap();
        assert!(Arc::ptr_eq(moved_child.mount(), &child_mount));
        assert_eq!(vfs_lookup(target_file).unwrap().inode(), child_file.inode());

        drop(moved_child);
        drop(child_mount);
        drop(target);
        drop(source);
        drop(child_file);
        vfs_unlink(target_file).unwrap();
        vfs_unmount(target_nested).unwrap();
        vfs_rmdir(target_nested).unwrap();
        vfs_unmount(target_dir).unwrap();
        vfs_rmdir(source_dir).unwrap();
        vfs_rmdir(target_dir).unwrap();
    }

    #[kunit]
    fn test_vfs_move_mount_lookup_generation_retry_returns_new_state() {
        let source_dir = Path::new("/kunit-vfs-move-retry-src");
        let source_file = Path::new("/kunit-vfs-move-retry-src/file");
        let target_dir = Path::new("/kunit-vfs-move-retry-target");
        let target_file = Path::new("/kunit-vfs-move-retry-target/file");

        vfs_mkdir(source_dir, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(target_dir, InodePerm::all_rwx()).unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            source_dir,
        )
        .unwrap();
        let file = vfs_touch(source_file, InodePerm::all_rwx()).unwrap();

        let source = vfs_lookup(source_dir).unwrap();
        let target = vfs_lookup(target_dir).unwrap();
        let moved_mount = source.mount().clone();

        let retried = crate::fs::namei::resolve_with_mount_retry_hook_for_kunit(
            target_file,
            ResolveFlags::empty(),
            || {
                move_mount(&source, &target).unwrap();
            },
        )
        .unwrap();

        assert!(Arc::ptr_eq(retried.mount(), &moved_mount));
        assert_eq!(retried.inode(), file.inode());
        assert_eq!(retried.to_string(), "/kunit-vfs-move-retry-target/file");
        assert_eq!(vfs_lookup(source_file).unwrap_err(), SysError::NotFound);

        drop(retried);
        drop(moved_mount);
        drop(target);
        drop(source);
        drop(file);
        vfs_unlink(target_file).unwrap();
        vfs_unmount(target_dir).unwrap();
        vfs_rmdir(source_dir).unwrap();
        vfs_rmdir(target_dir).unwrap();
    }

    #[kunit]
    fn test_vfs_move_mount_rejects_cycle_and_non_root_source() {
        let source_dir = Path::new("/kunit-vfs-move-cycle-src");
        let nested_dir = Path::new("/kunit-vfs-move-cycle-src/nested");
        let target_dir = Path::new("/kunit-vfs-move-cycle-target");

        vfs_mkdir(source_dir, InodePerm::all_rwx()).unwrap();
        vfs_mkdir(target_dir, InodePerm::all_rwx()).unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            source_dir,
        )
        .unwrap();
        vfs_mkdir(nested_dir, InodePerm::all_rwx()).unwrap();

        let source = vfs_lookup(source_dir).unwrap();
        let nested = vfs_lookup(nested_dir).unwrap();
        assert_eq!(
            move_mount(&source, &nested).unwrap_err(),
            SysError::InvalidArgument
        );

        let target = vfs_lookup(target_dir).unwrap();
        assert_eq!(
            move_mount(&nested, &target).unwrap_err(),
            SysError::InvalidArgument
        );

        drop(target);
        drop(nested);
        drop(source);
        vfs_rmdir(nested_dir).unwrap();
        vfs_unmount(source_dir).unwrap();
        vfs_rmdir(source_dir).unwrap();
        vfs_rmdir(target_dir).unwrap();
    }

    #[kunit]
    fn test_vfs_private_propagation_validates_live_target() {
        let mountpoint = Path::new("/kunit-vfs-private");
        let inner_dir = Path::new("/kunit-vfs-private/inner");
        let stale_dir = Path::new("/kunit-vfs-private-stale");

        vfs_mkdir(mountpoint, InodePerm::all_rwx()).unwrap();
        let stale = vfs_mkdir(stale_dir, InodePerm::all_rwx()).unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            mountpoint,
        )
        .unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            stale_dir,
        )
        .unwrap();
        vfs_mkdir(inner_dir, InodePerm::all_rwx()).unwrap();

        let mount_root = vfs_lookup(mountpoint).unwrap();
        let inner = vfs_lookup(inner_dir).unwrap();
        assert_eq!(make_mount_private(&mount_root, false).unwrap(), 1);
        assert_eq!(make_mount_private(&inner, true).unwrap(), 1);
        assert_eq!(
            make_mount_private(&stale, false).unwrap_err(),
            SysError::Busy
        );

        drop(inner);
        drop(mount_root);
        drop(stale);
        vfs_rmdir(inner_dir).unwrap();
        vfs_unmount(mountpoint).unwrap();
        vfs_unmount(stale_dir).unwrap();
        vfs_rmdir(mountpoint).unwrap();
        vfs_rmdir(stale_dir).unwrap();
    }

    #[kunit]
    fn test_vfs_remount_readonly_rechecks_existing_file_writes() {
        let mountpoint = Path::new("/kunit-vfs-remount-ro");
        let file_path = Path::new("/kunit-vfs-remount-ro/file");

        vfs_mkdir(mountpoint, InodePerm::all_rwx()).unwrap();
        vfs_mount_at(
            "ramfs",
            MountSource::Pseudo,
            MountAttrFlags::empty(),
            mountpoint,
        )
        .unwrap();

        let file = vfs_touch(file_path, InodePerm::all_rwx()).unwrap();
        let opened = vfs_open(file_path).unwrap();
        assert_eq!(opened.write(b"rw").unwrap(), 2);

        let mount_root = vfs_lookup(mountpoint).unwrap();
        remount_attrs(&mount_root, MountAttrFlags::RDONLY).unwrap();
        assert_eq!(opened.write(b"ro").unwrap_err(), SysError::ReadOnlyFs);
        assert_eq!(
            vfs_touch(Path::new("/kunit-vfs-remount-ro/new"), InodePerm::all_rwx()).unwrap_err(),
            SysError::ReadOnlyFs
        );

        remount_attrs(&mount_root, MountAttrFlags::empty()).unwrap();
        assert_eq!(opened.write(b"rw").unwrap(), 2);

        drop(opened);
        drop(file);

        vfs_unlink(file_path).unwrap();
        vfs_unmount(mountpoint).unwrap();
        vfs_rmdir(mountpoint).unwrap();
    }

    #[kunit]
    fn test_vfs_remount_rejects_plain_directory_target() {
        let dir = Path::new("/kunit-vfs-remount-dir");

        let path = vfs_mkdir(dir, InodePerm::all_rwx()).unwrap();
        assert_eq!(
            remount_attrs(&path, MountAttrFlags::RDONLY).unwrap_err(),
            SysError::InvalidArgument
        );

        vfs_rmdir(dir).unwrap();
    }
}
