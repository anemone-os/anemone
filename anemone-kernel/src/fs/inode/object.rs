use super::*;

/// Index Node, core abstraction of a file in VFS.
///
/// I think name this struct `Vnode` may sounds cooler? But `Inode` is more
/// traditional and less confusing, so let's stick to it.
pub(in crate::fs) struct Inode {
    ino: Ino,
    ty: InodeType,
    ops: &'static InodeOps,
    /// Weak to avoid circular reference. This can always be upgraded to strong
    /// when needed, ensured by the invariant of VFS.
    sb: Weak<SuperBlock>,
    prv: AnyOpaque,
    /// Number of active references. Separate from `Arc` strong count.
    /// The cache pool's `Arc` represents residency; this counter tracks
    /// business-level active usage.
    rc: AtomicUsize,
    /// Whether this inode is currently reachable from the superblock's ino
    /// index. Unlinked-but-still-alive inodes are resident ghosts with this
    /// flag cleared.
    indexed: AtomicBool,
    /// Logical memory mapping for this inode, if any.
    mapping: Option<Arc<dyn VmObject>>,
    /// Sole local whole-file flock grant and wait-notification domain.
    flock: FlockDomain,
    /// Sole local POSIX byte-range grant and conflict domain.
    posix_locks: PosixLockDomain,
    /// Cached metadata that can be updated by the inode's file operations
    /// without accesing underlying filesystem, thus speeding up common
    /// operations like `stat` and `write`.
    ///
    /// TODO: dirty flag
    meta: RwLock<InodeMeta>,
}

impl Debug for Inode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Inode")
            .field("ino", &self.ino)
            .field("ty", &self.ty)
            .field("rc", &self.rc.load(Ordering::Relaxed))
            .field("indexed", &self.indexed.load(Ordering::Relaxed))
            .finish()
    }
}

// No Drop impl — eviction is handled by explicit controlled paths only,
// never by the last Arc destructor. See `SuperBlock::evict` / `evict_all`.
//
// Handling eviction in Drop is definitely a bad design, cz we lost control over
// when it happens, and we might even end up in deadlocks if we're not careful
// enough!

macro_rules! gen_set_xtime {
    ($($time:ident),*) => {
        paste::paste! {
            $(
                pub(in crate::fs) fn [<set_ $time>](&self, time: Duration) {
                    self.meta.write().$time = time;
                }
            )*
        }
    };
}

impl Inode {
    /// Create a new inode, with:
    ///
    /// - 'meta' set to [InodeMeta::ZERO]
    ///
    /// With that being said, the newly created inode is not fully initialized
    /// until the caller sets the correct metadata and link count, and links it
    /// to the superblock's ino index if necessary. **So backend filesystem
    /// drivers are responsible for completing the initialization of
    /// [InodeMeta].**
    pub(in crate::fs) fn new(
        ino: Ino,
        ty: InodeType,
        ops: &'static InodeOps,
        sb: Arc<SuperBlock>,
        prv: AnyOpaque,
    ) -> Self {
        let meta = InodeMeta::ZERO;
        Self {
            ino,
            ty,
            ops,
            sb: Arc::downgrade(&sb),
            prv,
            rc: AtomicUsize::new(0),
            indexed: AtomicBool::new(false),
            mapping: None,
            flock: FlockDomain::new(),
            posix_locks: PosixLockDomain::new(),
            meta: RwLock::new(meta),
        }
    }

    pub(in crate::fs) const fn ino(&self) -> Ino {
        self.ino
    }

    pub(in crate::fs) fn nlink(&self) -> u64 {
        self.meta.read().nlink
    }

    /// This method can be used when we want to update multiple fields in `meta`
    /// at once, to avoid intermediate states that violate invariants.
    pub(in crate::fs) fn meta_snapshot(&self) -> InodeMeta {
        *self.meta.read()
    }

    pub(in crate::fs) fn indexed(&self) -> bool {
        self.indexed.load(Ordering::Acquire)
    }

    pub(in crate::fs) fn set_indexed(&self, indexed: bool) {
        self.indexed.store(indexed, Ordering::Release);
    }

    pub(in crate::fs) fn inc_nlink(&self) {
        self.meta.write().nlink += 1;
    }

    pub(in crate::fs) fn set_nlink(&self, nlink: u64) {
        self.meta.write().nlink = nlink;
    }

    /// See `meta_snapshot` for the rationale of this method.
    pub(in crate::fs) fn set_meta(&self, meta: &InodeMeta) {
        // avoid dereferencing here cz InodeMeta consumes too much stack space

        let mut m = self.meta.write();
        m.nlink = meta.nlink;
        m.size = meta.size;
        m.perm = meta.perm;
        m.uid = meta.uid;
        m.gid = meta.gid;
        m.atime = meta.atime;
        m.mtime = meta.mtime;
        m.ctime = meta.ctime;
    }

    #[track_caller]
    pub(in crate::fs) fn dec_nlink(&self) {
        let mut meta = self.meta.write();
        debug_assert!(meta.nlink > 0, "nlink underflow on inode {:?}", self.ino);
        meta.nlink -= 1;
    }

    /// Get the private data of this inode.
    pub(in crate::fs) fn prv(&self) -> &AnyOpaque {
        &self.prv
    }

    pub(in crate::fs) fn sb(&self) -> Arc<SuperBlock> {
        if let Some(sb) = self.sb.upgrade() {
            sb
        } else {
            panic!("inode's superblock has been dropped");
        }
    }

    pub(in crate::fs) fn mapping(&self) -> Option<&Arc<dyn VmObject>> {
        self.mapping.as_ref()
    }

    pub(in crate::fs) fn set_mapping(&mut self, mapping: Option<Arc<dyn VmObject>>) {
        self.mapping = mapping;
    }

    pub(in crate::fs) fn ty(&self) -> InodeType {
        self.ty
    }

    pub(in crate::fs) fn perm(&self) -> InodePerm {
        self.meta.read().perm
    }

    pub(in crate::fs) fn set_perm(&self, perm: InodePerm) {
        self.meta.write().perm = perm;
    }

    pub(in crate::fs) fn set_size(&self, size: u64) {
        self.meta.write().size = size;
    }

    pub(in crate::fs) fn update_size_max(&self, size: u64) {
        let mut meta = self.meta.write();
        meta.size = meta.size.max(size);
    }

    /// When more than one time field needs to be updated, it's better to update
    /// them in one shot to avoid intermediate states that violate invariants.
    pub(in crate::fs) fn set_times(&self, atime: Duration, mtime: Duration, ctime: Duration) {
        let mut meta = self.meta.write();
        meta.atime = atime;
        meta.mtime = mtime;
        meta.ctime = ctime;
    }

    pub(in crate::fs) fn chmod(&self, perm: InodePerm, ctime: Duration) {
        let mut meta = self.meta.write();
        meta.perm = perm;
        meta.ctime = ctime;
    }

    pub(in crate::fs) fn chown(&self, owner: Option<Uid>, group: Option<Gid>, ctime: Duration) {
        let mut meta = self.meta.write();
        // None means Linux's -1 no-change sentinel survived syscall decoding.
        if let Some(owner) = owner {
            meta.uid = owner;
        }
        if let Some(group) = group {
            meta.gid = group;
        }
        meta.ctime = ctime;
    }

    fn setid_drop_mask(
        ty: InodeType,
        perm: InodePerm,
        gid: Gid,
        checker: &FsPermChecker,
        modif: ModifType,
    ) -> InodePerm {
        let mut remove = InodePerm::empty();

        if ty != InodeType::Regular {
            return remove;
        }

        match modif {
            ModifType::Modify => {
                if checker.has_cap(Capability::FSETID) {
                    return remove;
                }
                if perm.contains(InodePerm::ISUID) {
                    remove.insert(InodePerm::ISUID);
                }
            },
            ModifType::Own => {
                if perm.contains(InodePerm::ISUID) {
                    remove.insert(InodePerm::ISUID);
                }
            },
        }

        if perm.contains(InodePerm::ISGID) {
            if perm.contains(InodePerm::IXGRP)
                || (!checker.fs_group_allowed(gid) && !checker.has_cap(Capability::FSETID))
            {
                remove.insert(InodePerm::ISGID);
            }
        }

        remove
    }

    /// Remove set-id privileges that must not survive a successful file
    /// content or ownership modification.
    pub(in crate::fs) fn after_modified(
        &self,
        cred: &CredentialSet,
        modif: ModifType,
        ctime: Duration,
    ) {
        let checker = FsPermChecker::new(cred.clone());
        let mut meta = self.meta.write();
        let remove = Self::setid_drop_mask(self.ty, meta.perm, meta.gid, &checker, modif);

        if remove.is_empty() {
            return;
        }

        meta.perm.remove(remove);
        meta.ctime = ctime;
    }

    gen_set_xtime!(atime, mtime, ctime);
}

impl Inode {
    /// Get the reference count of this inode.
    ///
    /// **Only Vfs itself can call this method. File system drivers should
    /// not.**
    pub(in crate::fs) fn rc(&self) -> usize {
        self.rc.load(Ordering::Relaxed)
    }

    /// Increment the reference count of this inode by 1.
    ///
    /// **Only Vfs itself can call this method. File system drivers should
    /// not.**
    pub(in crate::fs) fn inc_rc(&self) {
        self.rc.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement the reference count of this inode by 1, and return the
    /// previous value.
    ///
    /// **Only Vfs itself can call this method. File system drivers should
    /// not.**
    #[track_caller]
    pub(in crate::fs) fn dec_rc(&self) -> usize {
        let prev = self.rc.fetch_sub(1, Ordering::Relaxed);
        debug_assert!(prev > 0, "rc underflow on inode {:?}", self.ino);
        prev
    }
}

#[derive(Debug)]
pub struct InodeRef(Arc<Inode>);

impl InodeRef {
    /// Get the underlying inode.
    ///
    /// This operation is very dangerous and should be used with extreme
    /// caution. **It is intended for filesystem drivers and VFS only.**
    pub(in crate::fs) fn inode(&self) -> &Arc<Inode> {
        &self.0
    }
}

impl Drop for InodeRef {
    fn drop(&mut self) {
        self.inode().dec_rc();
    }
}

impl PartialEq for InodeRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(self.inode(), other.inode())
    }
}

impl Eq for InodeRef {}

impl Clone for InodeRef {
    fn clone(&self) -> Self {
        self.inode().inc_rc();
        Self(self.inode().clone())
    }
}

impl InodeRef {
    pub(in crate::fs) fn new(inode: Arc<Inode>) -> Self {
        inode.inc_rc();
        Self(inode)
    }

    pub(in crate::fs) fn flock_domain(&self) -> &FlockDomain {
        &self.inode().flock
    }

    pub(in crate::fs) fn posix_lock_domain(&self) -> &PosixLockDomain {
        &self.inode().posix_locks
    }

    /// Get the inode number.
    pub fn ino(&self) -> Ino {
        self.inode().ino
    }

    /// Get the inode type.
    pub fn ty(&self) -> InodeType {
        self.inode().ty
    }

    pub fn perm(&self) -> InodePerm {
        self.inode().meta.read().perm
    }

    pub fn mode(&self) -> InodeMode {
        InodeMode::new(self.ty(), self.perm())
    }

    pub fn nlink(&self) -> u64 {
        self.inode().nlink()
    }

    pub fn uid(&self) -> Uid {
        self.inode().meta.read().uid
    }

    pub fn gid(&self) -> Gid {
        self.inode().meta.read().gid
    }

    pub fn mapping(&self) -> Option<&Arc<dyn VmObject>> {
        self.inode().mapping()
    }

    pub fn size(&self) -> u64 {
        self.inode().meta.read().size
    }

    pub fn atime(&self) -> Duration {
        self.inode().meta.read().atime
    }

    pub fn mtime(&self) -> Duration {
        self.inode().meta.read().mtime
    }

    pub fn ctime(&self) -> Duration {
        self.inode().meta.read().ctime
    }

    pub fn chmod(&self, perm: InodePerm, ctime: Duration) {
        self.inode().chmod(perm, ctime);
    }

    pub fn chown(&self, owner: Option<Uid>, group: Option<Gid>, ctime: Duration) {
        self.inode().chown(owner, group, ctime);
    }

    pub fn set_times(&self, atime: Option<Duration>, mtime: Option<Duration>, ctime: Duration) {
        let mut meta = self.inode().meta.write();
        if let Some(atime) = atime {
            meta.atime = atime;
        }
        if let Some(mtime) = mtime {
            meta.mtime = mtime;
        }
        meta.ctime = ctime;
    }

    pub fn after_modified(&self, cred: &CredentialSet, modif: ModifType, ctime: Duration) {
        self.inode().after_modified(cred, modif, ctime);
    }

    /// Get the superblock that this inode belongs to.
    pub fn sb(&self) -> Arc<SuperBlock> {
        if let Some(sb) = self.inode().sb.upgrade() {
            sb
        } else {
            panic!("inode's superblock has been dropped");
        }
    }
}

// VTable operations re-exported here.
impl InodeRef {
    pub fn touch(&self, name: &str, perm: InodePerm) -> Result<InodeRef, SysError> {
        (self.inode().ops.touch)(self, name, perm)
    }

    pub fn make_node(
        &self,
        name: &str,
        description: MakeNodeDescription,
    ) -> Result<InodeRef, SysError> {
        (self.inode().ops.make_node)(self, name, description)
    }

    pub fn mkdir(&self, name: &str, perm: InodePerm) -> Result<InodeRef, SysError> {
        (self.inode().ops.mkdir)(self, name, perm)
    }

    pub fn symlink(&self, name: &str, target: &Path) -> Result<InodeRef, SysError> {
        (self.inode().ops.symlink)(self, name, target)
    }

    /// Lookup a child dentry under this inode by name.
    pub fn lookup(&self, name: &str) -> Result<InodeRef, SysError> {
        (self.inode().ops.lookup)(self, name)
    }

    pub fn link(&self, name: &str, target: &InodeRef) -> Result<(), SysError> {
        (self.inode().ops.link)(self, name, target)
    }

    pub fn unlink(&self, name: &str) -> Result<(), SysError> {
        (self.inode().ops.unlink)(self, name)
    }

    pub fn rmdir(&self, name: &str) -> Result<(), SysError> {
        (self.inode().ops.rmdir)(self, name)
    }

    pub fn rename(
        &self,
        old_name: &str,
        new_dir: &InodeRef,
        new_name: &str,
        flags: RenameFlags,
    ) -> Result<(), SysError> {
        (self.inode().ops.rename)(self, old_name, new_dir, new_name, flags)
    }

    /// Open this inode as a file and return an [OpenedFile] containing the file
    /// operations and private data, which will be used by VFS layer to create a
    /// [File] object finally.
    pub fn open(&self) -> Result<OpenedFile, SysError> {
        (self.inode().ops.open)(self)
    }

    pub fn truncate(&self, size: u64, cred: &CredentialSet) -> Result<(), SysError> {
        match self.ty() {
            InodeType::Dir => Err(SysError::IsDir),
            InodeType::Regular => {
                (self.inode().ops.truncate)(self, size)?;
                self.after_modified(cred, ModifType::Modify, Instant::now().to_duration());
                Ok(())
            },
            _ => Err(SysError::NotReg),
        }
    }

    pub fn read_link(&self) -> Result<PathBuf, SysError> {
        (self.inode().ops.read_link)(self)
    }

    pub fn get_attr(&self) -> Result<InodeStat, SysError> {
        (self.inode().ops.get_attr)(self)
    }

    pub fn get_file_cap(&self) -> Result<FileCapabilities, SysError> {
        Ok(FileCapabilities::empty())
    }
}
