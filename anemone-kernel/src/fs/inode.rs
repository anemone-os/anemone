use anemone_abi::fs::linux::{mode as linux_mode, stat::Stat as LinuxStat};
use core::{fmt::Debug, time::Duration};

use crate::{prelude::*, utils::any_opaque::AnyOpaque};

/// VTable an inode must implement to support file system operations.
///
/// Inodes have permission bits. But filesystem drivers are not expected to
/// check them by themselves. Instead, VFS will check them before calling these
/// operations.
pub struct InodeOps {
    pub lookup: fn(dir: &InodeRef, name: &str) -> Result<InodeRef, FsError>,

    pub touch: fn(dir: &InodeRef, name: &str, perm: InodePerm) -> Result<InodeRef, FsError>,

    pub mkdir: fn(dir: &InodeRef, name: &str, perm: InodePerm) -> Result<InodeRef, FsError>,

    pub symlink: fn(dir: &InodeRef, name: &str, target: &Path) -> Result<InodeRef, FsError>,

    pub link: fn(dir: &InodeRef, name: &str, target: &InodeRef) -> Result<(), FsError>,
    pub unlink: fn(dir: &InodeRef, name: &str) -> Result<(), FsError>,

    pub rmdir: fn(dir: &InodeRef, name: &str) -> Result<(), FsError>,

    /// Quoted from [Linux's VFS documentation](https://docs.kernel.org/filesystems/vfs.html):
    ///
    /// "
    /// open:
    /// called by the VFS when an inode should be opened. When the VFS opens a
    /// file, it creates a new “struct file”. It then calls the open method for
    /// the newly allocated file structure. **You might think that the open
    /// method really belongs in “struct inode_operations”, and you may be
    /// right.** I think it’s done the way it is because it makes
    /// filesystems simpler to implement. The open() method is a good place
    /// to initialize the “private_data” member in the file structure if you
    /// want to point to a device structure.
    /// "
    ///
    /// So we put this method here.
    pub open: fn(&InodeRef) -> Result<OpenedFile, FsError>,

    /// If this is a symlink, return the target path.
    pub read_link: fn(&InodeRef) -> Result<PathBuf, FsError>,

    /// Query inode metadata in a filesystem-neutral shape.
    pub get_attr: fn(&InodeRef) -> Result<InodeStat, FsError>,
}

pub struct OpenedFile {
    pub file_ops: &'static FileOps,
    pub prv: AnyOpaque,
}

/// Inode number type. Uniquely identifies an inode within a superblock.
///
/// **0 is reserved for invalid inode.** Valid inode numbers start from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ino(u64);

impl Ino {
    /// The invalid inode number, used to represent an error or uninitialized
    /// state.
    pub const INVALID: Self = Self(0);

    pub const fn try_new(value: u64) -> Result<Self, InoIsZero> {
        if value == 0 {
            Err(InoIsZero)
        } else {
            Ok(Self(value))
        }
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct InoIsZero;

impl TryFrom<u64> for Ino {
    type Error = InoIsZero;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InodeType {
    Regular,
    Dir,
    Dev,
    Symlink,
    Fifo,
}

impl InodeType {
    /// Convert to Linux's mode bits, with only file type bits set.
    pub const fn to_linux_mode_bits(self) -> u32 {
        match self {
            Self::Regular => linux_mode::S_IFREG,
            Self::Dir => linux_mode::S_IFDIR,
            Self::Dev => linux_mode::S_IFCHR,
            Self::Symlink => linux_mode::S_IFLNK,
            Self::Fifo => linux_mode::S_IFIFO,
        }
    }
}

bitflags! {
    /// Permission bits for an inode.
    ///
    /// We simply re-export Linux's permission bits here, since it's good enough.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct InodePerm: u16 {
        /// Set-user-ID on execution.
        const ISUID = linux_mode::S_ISUID as u16;
        /// Set-group-ID on execution.
        const ISGID = linux_mode::S_ISGID as u16;
        /// Sticky bit.
        const ISVTX = linux_mode::S_ISVTX as u16;

        /// Read permission, owner.
        const IRUSR = linux_mode::S_IRUSR as u16;
        /// Write permission, owner.
        const IWUSR = linux_mode::S_IWUSR as u16;
        /// Execute permission, owner.
        const IXUSR = linux_mode::S_IXUSR as u16;
        /// Read permission, group.
        const IRGRP = linux_mode::S_IRGRP as u16;
        /// Write permission, group.
        const IWGRP = linux_mode::S_IWGRP as u16;
        /// Execute permission, group.
        const IXGRP = linux_mode::S_IXGRP as u16;
        /// Read permission, others.
        const IROTH = linux_mode::S_IROTH as u16;
        /// Write permission, others.
        const IWOTH = linux_mode::S_IWOTH as u16;
        /// Execute permission, others.
        const IXOTH = linux_mode::S_IXOTH as u16;

        /// Shortcut for all read/write/execute permissions for owner.
        const RWXU = linux_mode::S_IRWXU as u16;
        /// Shortcut for all read/write/execute permissions for group.
        const RWXG = linux_mode::S_IRWXG as u16;
        /// Shortcut for all read/write/execute permissions for others.
        const RWXO = linux_mode::S_IRWXO as u16;
    }
}

impl InodePerm {
    /// All regular rwx permission bits, excluding suid/sgid/sticky.
    pub const fn all_rwx() -> Self {
        Self::RWXU.union(Self::RWXG).union(Self::RWXO)
    }

    pub const fn from_linux_bits(bits: u32) -> Option<Self> {
        // Since we just re-export linux's bits as our bits, conversion is fairly simple
        // here.
        Self::from_bits(bits as u16)
    }
}

/// Device number for inodes' underlying device.
///
/// For regular files and directories, this is `DeviceId::None`. For device
/// files, this is the actual device number.
///
/// This type is actually seldom used in kernel code. It's mainly for
/// compatibility with Linux's `st_dev` and `st_rdev` fields in `struct stat`,
/// which are exposed to userspace and expected to be in a certain format.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DeviceId {
    #[default]
    None,
    Char(CharDevNum),
    Block(BlockDevNum),
    Raw(u64),
}

impl DeviceId {
    /// Get the raw device number.
    pub fn raw(self) -> u64 {
        match self {
            Self::None => 0,
            Self::Char(devnum) => devnum.raw() as u64,
            Self::Block(devnum) => devnum.raw() as u64,
            Self::Raw(value) => value,
        }
    }
}

/// Unlike Linux's way, we explicit split file type and permission bits into two
/// fields, which is more clear and less error-prone.
///
/// This can be regarded as Linux's `mode_t`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InodeMode {
    ty: InodeType,
    perm: InodePerm,
}

impl InodeMode {
    pub const fn new(ty: InodeType, perm: InodePerm) -> Self {
        Self { ty, perm }
    }

    pub const fn new_with_all_perm(ty: InodeType) -> Self {
        Self {
            ty,
            perm: InodePerm::all_rwx(),
        }
    }

    pub const fn ty(self) -> InodeType {
        self.ty
    }

    pub const fn perm(self) -> InodePerm {
        self.perm
    }

    /// Convert to Linux's mode bits.
    pub const fn to_linux_mode(self) -> u32 {
        self.ty.to_linux_mode_bits() | self.perm.bits() as u32
    }

    pub fn from_linux_mode(mode: u32) -> Option<Self> {
        let ty = match mode & linux_mode::S_IFMT {
            linux_mode::S_IFREG => InodeType::Regular,
            linux_mode::S_IFDIR => InodeType::Dir,
            linux_mode::S_IFCHR | linux_mode::S_IFBLK => InodeType::Dev,
            linux_mode::S_IFLNK => InodeType::Symlink,
            linux_mode::S_IFIFO => InodeType::Fifo,
            _ => {
                // catch unknown file types early.
                knoticeln!("unknown inode type in linux mode: {:o}", mode);
                return None;
            },
        };
        let perm = InodePerm::from_bits_truncate(mode as u16);
        Some(Self { ty, perm })
    }
}

/// Metadata of an inode, in a filesystem-neutral shape.
///
/// Each filesystem must at least store these fields.
///
/// TODO: some fields are set to dummy values for now, since we haven't
/// implemented all needed features.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InodeStat {
    /// Device ID of the filesystem this inode belongs to.
    pub fs_dev: DeviceId,
    pub ino: Ino,
    pub mode: InodeMode,
    pub nlink: u64,
    pub uid: u32,
    pub gid: u32,
    /// Note the difference between `fs_dev` and `rdev`.
    pub rdev: DeviceId,
    /// Idk why Linux uses i64 for this field. We'll use u64 here.
    pub size: u64,
    /// Access time.
    pub atime: Duration,
    /// Modification time.
    pub mtime: Duration,
    /// Status change time.
    pub ctime: Duration,
}

impl InodeStat {
    /// This, in fact, is not the block size of either the block size of
    /// underlying storage or the IO block size of the filesystem. It's the
    /// "preferred block size for efficient filesystem I/O", which is a very
    /// vague concept and can be decided by each filesystem itself. For
    /// simplicity we just set it to 4096 for all filesystems.
    pub const fn linux_blksize() -> i32 {
        4096
    }

    /// No matter what the actual block size of the underlying storage is, value
    /// of this field is always calculated, according to POSIX, as `(size + 511)
    /// / 512`, i.e., the number of 512-byte blocks this file occupies.
    pub const fn linux_blocks(self) -> i64 {
        ((self.size + 511) / 512) as i64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InodeMeta {
    /// Link count is inode-local metadata. Multi-object atomicity is provided
    /// by filesystem transaction locks, not by exposing an inode inner lock.
    pub nlink: u64,
    /// Size of file in bytes.
    pub size: u64,
    /// Permission bits for this inode.
    pub perm: InodePerm,
    /// Access time.
    pub atime: Duration,
    /// Modification time.
    pub mtime: Duration,
    /// Status change time.
    pub ctime: Duration,
}

impl InodeMeta {
    pub const ZERO: Self = Self {
        nlink: 0,
        size: 0,
        perm: InodePerm::empty(),
        atime: Duration::ZERO,
        mtime: Duration::ZERO,
        ctime: Duration::ZERO,
    };
}

impl InodeStat {
    /// Convert to Linux's `struct stat`.
    pub fn to_linux_stat(self) -> LinuxStat {
        LinuxStat {
            st_dev: self.fs_dev.raw(),
            st_ino: self.ino.get(),
            st_mode: self.mode.to_linux_mode(),
            st_nlink: self.nlink.min(u32::MAX as u64) as u32,
            st_uid: self.uid,
            st_gid: self.gid,
            st_rdev: self.rdev.raw(),
            __pad1: 0,
            st_size: self.size as i64,
            st_blksize: Self::linux_blksize(),
            __pad2: 0,
            st_blocks: self.linux_blocks(),
            st_atime: self.atime.as_secs() as i64,
            st_atime_nsec: self.atime.subsec_nanos() as u64,
            st_mtime: self.mtime.as_secs() as i64,
            st_mtime_nsec: self.mtime.subsec_nanos() as u64,
            st_ctime: self.ctime.as_secs() as i64,
            st_ctime_nsec: self.ctime.subsec_nanos() as u64,
            __unused: [0; 2],
        }
    }
}

impl From<InodeStat> for LinuxStat {
    fn from(value: InodeStat) -> Self {
        value.to_linux_stat()
    }
}

/// Index Node, core abstraction of a file in VFS.
///
/// I think name this struct `Vnode` may sounds cooler? But `Inode` is more
/// traditional and less confusing, so let's stick to it.
pub(super) struct Inode {
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
                pub(super) fn [<set_ $time>](&self, time: Duration) {
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
    pub(super) fn new(
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
            meta: RwLock::new(meta),
        }
    }

    pub(super) const fn ino(&self) -> Ino {
        self.ino
    }

    pub(super) fn nlink(&self) -> u64 {
        self.meta.read().nlink
    }

    /// This method can be used when we want to update multiple fields in `meta`
    /// at once, to avoid intermediate states that violate invariants.
    pub(super) fn meta_snapshot(&self) -> InodeMeta {
        *self.meta.read()
    }

    pub(super) fn indexed(&self) -> bool {
        self.indexed.load(Ordering::Acquire)
    }

    pub(super) fn set_indexed(&self, indexed: bool) {
        self.indexed.store(indexed, Ordering::Release);
    }

    pub(super) fn inc_nlink(&self) {
        self.meta.write().nlink += 1;
    }

    pub(super) fn set_nlink(&self, nlink: u64) {
        self.meta.write().nlink = nlink;
    }

    /// See `meta_snapshot` for the rationale of this method.
    pub(super) fn set_meta(&self, meta: &InodeMeta) {
        // avoid dereferencing here cz InodeMeta consumes too much stack space

        let mut m = self.meta.write();
        m.nlink = meta.nlink;
        m.size = meta.size;
        m.perm = meta.perm;
        m.atime = meta.atime;
        m.mtime = meta.mtime;
        m.ctime = meta.ctime;
    }

    #[track_caller]
    pub(super) fn dec_nlink(&self) {
        let mut meta = self.meta.write();
        debug_assert!(meta.nlink > 0, "nlink underflow on inode {:?}", self.ino);
        meta.nlink -= 1;
    }

    /// Get the private data of this inode.
    pub(super) fn prv(&self) -> &AnyOpaque {
        &self.prv
    }

    pub(super) fn sb(&self) -> Arc<SuperBlock> {
        if let Some(sb) = self.sb.upgrade() {
            sb
        } else {
            panic!("inode's superblock has been dropped");
        }
    }

    pub(super) fn ty(&self) -> InodeType {
        self.ty
    }

    pub(super) fn perm(&self) -> InodePerm {
        self.meta.read().perm
    }

    pub(super) fn set_perm(&self, perm: InodePerm) {
        self.meta.write().perm = perm;
    }

    pub(super) fn set_size(&self, size: u64) {
        self.meta.write().size = size;
    }

    pub(super) fn update_size_max(&self, size: u64) {
        let mut meta = self.meta.write();
        meta.size = meta.size.max(size);
    }

    /// When more than one time field needs to be updated, it's better to update
    /// them in one shot to avoid intermediate states that violate invariants.
    pub(super) fn set_times(&self, atime: Duration, mtime: Duration, ctime: Duration) {
        let mut meta = self.meta.write();
        meta.atime = atime;
        meta.mtime = mtime;
        meta.ctime = ctime;
    }

    gen_set_xtime!(atime, mtime, ctime);
}

impl Inode {
    /// Get the reference count of this inode.
    ///
    /// **Only Vfs itself can call this method. File system drivers should
    /// not.**
    pub(super) fn rc(&self) -> usize {
        self.rc.load(Ordering::Relaxed)
    }

    /// Increment the reference count of this inode by 1.
    ///
    /// **Only Vfs itself can call this method. File system drivers should
    /// not.**
    pub(super) fn inc_rc(&self) {
        self.rc.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement the reference count of this inode by 1, and return the
    /// previous value.
    ///
    /// **Only Vfs itself can call this method. File system drivers should
    /// not.**
    #[track_caller]
    pub(super) fn dec_rc(&self) -> usize {
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
    pub(super) fn inode(&self) -> &Arc<Inode> {
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
    pub(super) fn new(inode: Arc<Inode>) -> Self {
        inode.inc_rc();
        Self(inode)
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
    pub fn touch(&self, name: &str, perm: InodePerm) -> Result<InodeRef, FsError> {
        (self.inode().ops.touch)(self, name, perm)
    }

    pub fn mkdir(&self, name: &str, perm: InodePerm) -> Result<InodeRef, FsError> {
        (self.inode().ops.mkdir)(self, name, perm)
    }

    pub fn symlink(&self, name: &str, target: &Path) -> Result<InodeRef, FsError> {
        (self.inode().ops.symlink)(self, name, target)
    }

    /// Lookup a child dentry under this inode by name.
    pub fn lookup(&self, name: &str) -> Result<InodeRef, FsError> {
        (self.inode().ops.lookup)(self, name)
    }

    pub fn link(&self, name: &str, target: &InodeRef) -> Result<(), FsError> {
        (self.inode().ops.link)(self, name, target)
    }

    pub fn unlink(&self, name: &str) -> Result<(), FsError> {
        (self.inode().ops.unlink)(self, name)
    }

    pub fn rmdir(&self, name: &str) -> Result<(), FsError> {
        (self.inode().ops.rmdir)(self, name)
    }

    /// Open this inode as a file and return an [OpenedFile] containing the file
    /// operations and private data, which will be used by VFS layer to create a
    /// [File] object finally.
    pub fn open(&self) -> Result<OpenedFile, FsError> {
        (self.inode().ops.open)(self)
    }

    pub fn read_link(&self) -> Result<PathBuf, FsError> {
        (self.inode().ops.read_link)(self)
    }

    pub fn get_attr(&self) -> Result<InodeStat, FsError> {
        (self.inode().ops.get_attr)(self)
    }
}
