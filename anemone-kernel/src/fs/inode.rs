use core::fmt::Debug;

use crate::{prelude::*, utils::any_opaque::AnyOpaque};

/// VTable an inode must implement to support file system operations.
pub struct InodeOps {
    pub lookup: fn(dir: &InodeRef, name: &str) -> Result<InodeRef, FsError>,

    pub create: fn(dir: &InodeRef, name: &str, ty: InodeType) -> Result<InodeRef, FsError>,

    pub link: fn(dir: &InodeRef, name: &str, target: &InodeRef) -> Result<(), FsError>,
    pub unlink: fn(dir: &InodeRef, name: &str) -> Result<(), FsError>,

    pub mkdir: fn(dir: &InodeRef, name: &str) -> Result<InodeRef, FsError>,
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
    // Symlink is not supported yet.
}

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
    /// Link count is inode-local metadata. Multi-object atomicity is provided
    /// by filesystem transaction locks, not by exposing an inode inner lock.
    nlink: AtomicU64,
}

impl PartialEq for Inode {
    fn eq(&self, other: &Self) -> bool {
        self.ino == other.ino && self.sb.ptr_eq(&other.sb)
    }
}

impl Debug for Inode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Inode")
            .field("ino", &self.ino)
            .field("ty", &self.ty)
            .field("rc", &self.rc.load(Ordering::Relaxed))
            .finish()
    }
}

// No Drop impl — eviction is handled by explicit controlled paths only,
// never by the last Arc destructor. See `SuperBlock::evict` / `evict_all`.

impl Inode {
    /// Create a new inode, with:
    ///
    /// - `nlink` initialized to 1
    pub(super) fn new(
        ino: Ino,
        ty: InodeType,
        ops: &'static InodeOps,
        sb: Arc<SuperBlock>,
        prv: AnyOpaque,
    ) -> Self {
        Self {
            ino,
            ty,
            ops,
            sb: Arc::downgrade(&sb),
            prv,
            rc: AtomicUsize::new(0),
            nlink: AtomicU64::new(1),
        }
    }

    pub(super) const fn ino(&self) -> Ino {
        self.ino
    }

    pub(super) fn nlink(&self) -> u64 {
        self.nlink.load(Ordering::Acquire)
    }

    pub(super) fn inc_nlink(&self) {
        self.nlink.fetch_add(1, Ordering::AcqRel);
    }

    pub(super) fn dec_nlink(&self) {
        let prev = self
            .nlink
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |nlink| {
                nlink.checked_sub(1)
            });
        debug_assert!(prev.is_ok(), "nlink underflow on inode {:?}", self.ino);
    }

    /// Get the private data of this inode.
    pub(super) fn prv(&self) -> &AnyOpaque {
        &self.prv
    }
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

    pub fn nlink(&self) -> u64 {
        self.inode().nlink()
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
    /// Create a new dentry under this inode with the given name and type.
    pub fn create(&self, name: &str, ty: InodeType) -> Result<InodeRef, FsError> {
        (self.inode().ops.create)(self, name, ty)
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

    pub fn mkdir(&self, name: &str) -> Result<InodeRef, FsError> {
        (self.inode().ops.mkdir)(self, name)
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
}
