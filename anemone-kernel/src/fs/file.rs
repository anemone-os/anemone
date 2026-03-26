use crate::{prelude::*, utils::any_opaque::AnyOpaque};

/// VTable a file must implement to support file operations.
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

#[derive(Debug)]
pub struct DirContext {
    offset: usize,
}

impl DirContext {
    pub fn new() -> Self {
        Self { offset: 0 }
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn advance(&mut self, n: usize) {
        self.offset += n;
    }
}

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

    pub(super) fn inode(&self) -> &InodeRef {
        self.path.inode()
    }

    pub(super) fn set_pos(&self, pos: usize) {
        self.pos.store(pos, Ordering::Relaxed);
    }
}

impl File {
    pub fn pos(&self) -> usize {
        self.pos.load(Ordering::Relaxed)
    }
}

// VTable operations re-exported here.
impl File {
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, FsError> {
        (self.ops.read)(self, buf)
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, FsError> {
        (self.ops.write)(self, buf)
    }

    pub fn seek(&self, pos: usize) -> Result<(), FsError> {
        (self.ops.seek)(self, pos)
    }

    pub fn iterate(&self, ctx: &mut DirContext) -> Result<DirEntry, FsError> {
        (self.ops.iterate)(self, ctx)
    }
}
