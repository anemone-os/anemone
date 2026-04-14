use crate::{prelude::*, utils::any_opaque::AnyOpaque};

/// VTable a file must implement to support file operations.
#[derive(Debug)]
pub struct FileOps {
    pub read: fn(&File, buf: &mut [u8]) -> Result<usize, FsError>,
    pub write: fn(&File, buf: &[u8]) -> Result<usize, FsError>,
    pub seek: fn(&File, pos: usize) -> Result<(), FsError>,

    pub iterate: fn(&File, ctx: &mut DirContext) -> Result<DirEntry, FsError>,
}

#[derive(Debug)]
pub struct DirEntry {
    pub name: String,
    pub ino: Ino,
    pub ty: InodeType,
}

/// `DirContext` maintains a cursor internally.
///
/// **Users should not make any assumptions about the cursor's value. Instead,
/// the whole struct should be treated as an opaque handle.**
#[derive(Debug)]
pub struct DirContext {
    offset: usize,
}

impl DirContext {
    /// Create a new `DirContext` for iterating a directory from the beginning.
    pub fn new() -> Self {
        Self { offset: 0 }
    }

    /// Rebuild a `DirContext` from an opaque cursor previously produced by the
    /// same filesystem.
    ///
    /// **Only the VFS layer should use this method.**
    pub(super) fn from_offset(cursor: usize) -> Self {
        Self { offset: cursor }
    }

    /// Get current offset.
    ///
    /// **Only filesystem drivers and VFS layer should use this method.**
    pub(super) fn offset(&self) -> usize {
        self.offset
    }

    /// Advance the offset by `n`, whose meaning is defined by the filesystem
    /// driver.
    ///
    /// **Only filesystem drivers should use this method.**
    pub(super) fn advance(&mut self, n: usize) {
        self.offset += n;
    }
}

#[derive(Debug)]
pub struct File {
    path: PathRef,
    ops: &'static FileOps,
    prv: AnyOpaque,
    pos: AtomicUsize,
}

impl File {
    pub(super) fn new(path: PathRef, ops: &'static FileOps, prv: AnyOpaque) -> Self {
        Self {
            path,
            ops,
            prv,
            pos: AtomicUsize::new(0),
        }
    }

    pub(super) fn prv(&self) -> &AnyOpaque {
        &self.prv
    }

    /// The offset is an opaque value that can only be interpreted by the
    /// filesystem driver and vfs layer.
    ///
    /// For example, for regular files, it often represents the byte offset from
    /// the beginning of the file, which is maintained by filesystem drivers and
    /// vfs layer together.
    ///
    /// While for directories, it only represents the index of the next
    /// directory entry to read, which is only maintained by vfs layer.
    pub(super) fn set_pos(&self, pos: usize) {
        self.pos.store(pos, Ordering::Relaxed);
    }

    /// Get a `DirContext` for iterating the directory represented by this file.
    ///
    /// Returns an error if this file does not represent a directory.
    pub(super) fn dir_context(&self) -> Result<DirContext, FsError> {
        if self.path.inode().ty() != InodeType::Dir {
            return Err(FsError::NotDir);
        }
        Ok(DirContext::from_offset(self.pos()))
    }

    /// Commit the state of a `DirContext` after iterating a directory entry.
    /// This should be called by the VFS layer after a successful `iterate`
    /// call to update the file's cursor.
    ///
    /// Returns an error if this file does not represent a directory.
    pub(super) fn commit_dir_context(&self, ctx: &DirContext) -> Result<(), FsError> {
        if self.path.inode().ty() != InodeType::Dir {
            return Err(FsError::NotDir);
        }

        self.set_pos(ctx.offset());
        Ok(())
    }
}

impl File {
    pub fn pos(&self) -> usize {
        self.pos.load(Ordering::Relaxed)
    }
}

// VTable operations re-exported here.
impl File {
    pub fn inode(&self) -> &InodeRef {
        self.path.inode()
    }

    pub fn path(&self) -> &PathRef {
        &self.path
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize, FsError> {
        if buf.len() == 0 {
            return Ok(0);
        }

        (self.ops.read)(self, buf)
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, FsError> {
        if buf.len() == 0 {
            return Ok(0);
        }

        (self.ops.write)(self, buf)
    }

    pub fn seek(&self, pos: usize) -> Result<(), FsError> {
        (self.ops.seek)(self, pos)
    }

    pub fn iterate(&self, ctx: &mut DirContext) -> Result<DirEntry, FsError> {
        (self.ops.iterate)(self, ctx)
    }

    pub fn get_attr(&self) -> Result<InodeStat, FsError> {
        self.inode().get_attr()
    }
}
