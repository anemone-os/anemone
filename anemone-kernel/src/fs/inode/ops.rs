use super::*;

/// VTable an inode must implement to support file system operations.
///
/// Inodes have permission bits. But filesystem drivers are not expected to
/// check them by themselves. Instead, VFS will check them before calling these
/// operations.
pub struct InodeOps {
    pub lookup: fn(dir: &InodeRef, name: &str) -> Result<InodeRef, SysError>,

    pub touch: fn(dir: &InodeRef, name: &str, perm: InodePerm) -> Result<InodeRef, SysError>,

    pub mkdir: fn(dir: &InodeRef, name: &str, perm: InodePerm) -> Result<InodeRef, SysError>,

    pub symlink: fn(dir: &InodeRef, name: &str, target: &Path) -> Result<InodeRef, SysError>,

    pub link: fn(dir: &InodeRef, name: &str, target: &InodeRef) -> Result<(), SysError>,
    pub unlink: fn(dir: &InodeRef, name: &str) -> Result<(), SysError>,

    pub rmdir: fn(dir: &InodeRef, name: &str) -> Result<(), SysError>,

    pub rename: fn(
        old_dir: &InodeRef,
        old_name: &str,
        new_dir: &InodeRef,
        new_name: &str,
        flags: RenameFlags,
    ) -> Result<(), SysError>,

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
    pub open: fn(&InodeRef) -> Result<OpenedFile, SysError>,

    /// Change the logical size of a regular file.
    ///
    /// Filesystems are expected to update cached metadata and keep any
    /// resident file pages coherent enough for subsequent VFS reads.
    pub truncate: fn(&InodeRef, size: u64) -> Result<(), SysError>,

    /// If this is a symlink, return the target path.
    pub read_link: fn(&InodeRef) -> Result<PathBuf, SysError>,

    /// Query inode metadata in a filesystem-neutral shape.
    pub get_attr: fn(&InodeRef) -> Result<InodeStat, SysError>,
}

pub struct OpenedFile {
    pub file_ops: &'static FileOps,
    /// Open-time VFS behavior for the resulting file object.
    ///
    /// The empty default keeps ordinary VFS cursor semantics; stream-like
    /// objects must opt in explicitly at their open boundary.
    pub mode: FileMode,
    pub prv: AnyOpaque,
}

impl OpenedFile {
    pub fn new(file_ops: &'static FileOps, prv: AnyOpaque) -> Self {
        Self::with_mode(file_ops, FileMode::empty(), prv)
    }

    pub fn with_mode(file_ops: &'static FileOps, mode: FileMode, prv: AnyOpaque) -> Self {
        Self {
            file_ops,
            mode,
            prv,
        }
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct RenameFlags: u32 {
        const NO_REPLACE = 0x1;
    }
}

impl RenameFlags {
    /// Kept as a uniform call site for now, even though currently there is
    /// only one supported flag.
    pub fn validate(&self) -> Result<(), SysError> {
        Ok(())
    }
}
