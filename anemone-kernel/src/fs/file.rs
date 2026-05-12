use crate::{prelude::*, utils::any_opaque::AnyOpaque};

/// VTable a file must implement to support file operations.
#[derive(Debug)]
pub struct FileOps {
    pub read: fn(&File, pos: &mut usize, buf: &mut [u8]) -> Result<usize, SysError>,
    pub write: fn(&File, pos: &mut usize, buf: &[u8]) -> Result<usize, SysError>,
    pub validate_seek: fn(&File, pos: usize) -> Result<(), SysError>,

    /// Read a batch of directory entries starting at `pos` into `sink`.
    ///
    /// Return `ReadDirResult::Progressed` when this call successfully hands at
    /// least one new entry to the sink. Return `ReadDirResult::Eof` only when
    /// the directory is already exhausted before any new entry is accepted.
    pub read_dir:
        fn(&File, pos: &mut usize, sink: &mut dyn DirSink) -> Result<ReadDirResult, SysError>,
}

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub ino: Ino,
    pub ty: InodeType,
}

#[derive(Debug, Clone, Copy)]
pub enum ReadDirResult {
    /// At least one new directory entry was accepted by the sink.
    Progressed,
    /// The directory was already exhausted before any new entry was accepted.
    Eof,
}

#[derive(Debug, Clone, Copy)]
pub enum SinkResult {
    /// The sink accepted this entry, so the producer may advance its cursor.
    Accepted,
    /// The sink wants to stop before consuming this entry.
    ///
    /// Producers must not advance the directory cursor for the current entry.
    /// Sinks that cannot accept even the first entry of a batch should return
    /// [ReadDirResult::Eof] instead of [SinkResult::Stop].
    Stop,
}

/// Trait instead of concrete struct thus allowing more flexible
/// implementations. e.g. fixed-capacity array, zero-copy buffer, etc.
pub trait DirSink {
    fn push(&mut self, entry: DirEntry) -> Result<SinkResult, SysError>;
}

#[derive(Debug, Clone)]
pub struct FixedSizeDirSink<const N: usize> {
    entries: Vec<DirEntry>,
}

impl<const N: usize> FixedSizeDirSink<N> {
    pub fn new() -> Self {
        const_assert!(N > 0, "FixedSizeDirSink must have positive capacity");

        Self {
            entries: Vec::new(),
        }
    }

    pub fn entries(&self) -> &[DirEntry] {
        &self.entries
    }

    pub fn entries_mut(&mut self) -> &mut [DirEntry] {
        &mut self.entries
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl<const N: usize> DirSink for FixedSizeDirSink<N> {
    fn push(&mut self, entry: DirEntry) -> Result<SinkResult, SysError> {
        if self.entries.len() < N {
            self.entries.push(entry);
            Ok(SinkResult::Accepted)
        } else {
            Ok(SinkResult::Stop)
        }
    }
}

#[derive(Debug)]
pub struct File {
    path: PathRef,
    ops: &'static FileOps,
    prv: AnyOpaque,
    pos: Mutex<usize>,
}

impl File {
    pub(super) fn new(path: PathRef, ops: &'static FileOps, prv: AnyOpaque) -> Self {
        Self {
            path,
            ops,
            prv,
            pos: Mutex::new(0),
        }
    }

    pub(super) fn prv(&self) -> &AnyOpaque {
        &self.prv
    }
}

impl File {
    pub fn pos(&self) -> usize {
        *self.pos.lock()
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

    pub fn read(&self, buf: &mut [u8]) -> Result<usize, SysError> {
        if buf.len() == 0 {
            return Ok(0);
        }

        let mut pos = self.pos.lock();
        let read = (self.ops.read)(self, &mut *pos, buf)?;

        Ok(read)
    }

    /// Reading at specified offset without changing the file cursor.
    pub fn read_at(&self, pos: usize, buf: &mut [u8]) -> Result<usize, SysError> {
        if buf.len() == 0 {
            return Ok(0);
        }

        let mut dummy_pos = pos;
        let read = (self.ops.read)(self, &mut dummy_pos, buf)?;

        Ok(read)
    }

    pub fn read_exact(&self, mut buf: &mut [u8]) -> Result<(), SysError> {
        if buf.len() == 0 {
            return Ok(());
        }

        let mut pos = self.pos.lock();
        while !buf.is_empty() {
            let read = (self.ops.read)(self, &mut *pos, buf)?;
            if read == 0 {
                return Err(SysError::UnexpectedEof);
            }
            buf = &mut buf[read..];
        }

        Ok(())
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, SysError> {
        if buf.len() == 0 {
            return Ok(0);
        }

        let mut pos = self.pos.lock();

        let written = (self.ops.write)(self, &mut *pos, buf)?;

        Ok(written)
    }

    /// Writing at specified offset without changing the file cursor.
    pub fn write_at(&self, pos: usize, buf: &[u8]) -> Result<usize, SysError> {
        if buf.len() == 0 {
            return Ok(0);
        }

        let mut dummy_pos = pos;
        let written = (self.ops.write)(self, &mut dummy_pos, buf)?;

        Ok(written)
    }

    pub fn write_all(&self, mut buf: &[u8]) -> Result<(), SysError> {
        if buf.len() == 0 {
            return Ok(());
        }

        let mut pos = self.pos.lock();
        while !buf.is_empty() {
            let written = (self.ops.write)(self, &mut *pos, buf)?;
            if written == 0 {
                // TODO: EIO here is not that accurate.
                knoticeln!(
                    "write returned 0, but there's still data to write. treating it as an IO error"
                );
                return Err(SysError::IO);
            }
            buf = &buf[written..];
        }

        Ok(())
    }

    /// Different from [Self::seek] + [Self::write], this is an atomic
    /// operation.
    pub fn append(&self, buf: &[u8]) -> Result<usize, SysError> {
        let mut pos = self.pos.lock();
        let sz = self.inode().get_attr()?.size as usize;
        *pos = sz;
        let written = (self.ops.write)(self, &mut *pos, buf)?;
        Ok(written)
    }

    pub fn seek(&self, pos: usize) -> Result<(), SysError> {
        (self.ops.validate_seek)(self, pos)?;
        *self.pos.lock() = pos;
        Ok(())
    }

    pub fn read_dir(&self, sink: &mut dyn DirSink) -> Result<ReadDirResult, SysError> {
        let mut pos = self.pos.lock();
        (self.ops.read_dir)(self, &mut *pos, sink)
    }

    pub fn get_attr(&self) -> Result<InodeStat, SysError> {
        self.inode().get_attr()
    }
}
